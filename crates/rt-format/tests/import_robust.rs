// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Hostile input: mutated corpus files and pathological inputs must never panic or hang;
//! every importer either imports or refuses with an error. Deterministic (fixed seed).

use std::path::{Path, PathBuf};

use rt_format::import::{ImportError, ImportFormat, MAX_INPUT_BYTES, SourceFile, import_files};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/import")
}

/// Small files of every source (large ones make the test slow without adding cases).
fn samples() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for source in [
        "postman", "insomnia", "openapi", "har", "http", "curl", "invalid",
    ] {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(corpus().join(source))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.is_file())
            .collect();
        paths.sort();
        for p in paths {
            let data = std::fs::read(&p).unwrap();
            if data.len() <= 64 * 1024 {
                out.push((p.file_name().unwrap().to_string_lossy().into_owned(), data));
            }
        }
    }
    out
}

/// xorshift64*, enough for reproducible mutations.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const NOISE: &[&[u8]] = &[
    b"{",
    b"}",
    b"[",
    b"]",
    b"\"",
    b":",
    b",",
    b"\n",
    b"- ",
    b"  ",
    b"{{",
    b"}}",
    b"{%",
    b"%}",
    b"\\",
    b"'",
    b"$'",
    b"###",
    b"\n###\n",
    b"&",
    b"|",
    b"^",
    b"\xff",
    b"\xc3",
    b"\x00",
    b"null",
    b"*a",
    b"&a ",
    b"!!binary ",
    b"? ",
    b"#",
    b"\r",
    b"\t",
    b"1e999",
    b"-0",
    b"\"$ref\": \"#/a\"",
];

fn mutate(data: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut d = data.to_vec();
    for _ in 0..=rng.below(4) {
        let at = rng.below(d.len() + 1);
        match rng.below(4) {
            0 => d.truncate(at),
            1 => {
                let noise = NOISE[rng.below(NOISE.len())];
                d.splice(at..at, noise.iter().copied());
            }
            2 if !d.is_empty() => {
                let end = (at + rng.below(32)).min(d.len());
                d.drain(at.min(end)..end);
            }
            _ if !d.is_empty() => {
                let i = rng.below(d.len());
                d[i] = (rng.next() & 0xff) as u8;
            }
            _ => {}
        }
    }
    d
}

fn formats() -> Vec<Option<ImportFormat>> {
    std::iter::once(None)
        .chain(ImportFormat::ALL.iter().copied().map(Some))
        .collect()
}

/// Imports without panicking; a successful import must also be consistent.
fn check(label: &str, format: Option<ImportFormat>, name: &str, data: &[u8]) {
    let files = [SourceFile { name, data }];
    let result = std::panic::catch_unwind(|| import_files(format, &files));
    match result {
        Err(_) => panic!("{label}: panicked (format {format:?}, file {name})"),
        Ok(Ok(import)) => {
            assert!(
                import.files.files.contains_key("opencollection.yml"),
                "{label}: no opencollection.yml"
            );
            let read = rt_format::read_collection(&import.files)
                .unwrap_or_else(|e| panic!("{label}: written collection unreadable: {e}"));
            assert!(
                read.issues().is_empty(),
                "{label}: written collection has issues: {:?}",
                read.issues()
            );
        }
        Ok(Err(e)) => assert!(!e.to_string().is_empty(), "{label}: empty error"),
    }
}

#[test]
fn mutated_corpus_files_never_panic() {
    let samples = samples();
    assert!(samples.len() > 50, "corpus not found");
    let mut rng = Rng(0x5eed_1234_abcd_0001);
    let formats = formats();
    let rounds: usize = std::env::var("RT_IMPORT_FUZZ_ROUNDS")
        .ok()
        .and_then(|r| r.parse().ok())
        .unwrap_or(6);
    for (name, data) in &samples {
        for round in 0..rounds {
            let mutated = mutate(data, &mut rng);
            let format = formats[rng.below(formats.len())];
            check(&format!("{name} round {round}"), format, name, &mutated);
        }
    }
}

#[test]
fn every_format_survives_any_corpus_file() {
    // Forcing the wrong format on a real file is a common user mistake.
    for (name, data) in samples() {
        for format in formats() {
            check(&name, format, &name, &data);
        }
    }
}

#[test]
fn pathological_inputs_are_refused_quickly() {
    let deep_json = format!("{}{}", "[".repeat(1_000_000), "]".repeat(1_000_000));
    let deep_yaml = format!(
        "openapi: 3.0.0\ninfo: {}1{}\n",
        "{a: ".repeat(100_000),
        "}".repeat(100_000)
    );
    let anchors = {
        // Billion laughs: aliases must not be expanded exponentially.
        let mut s = String::from("openapi: 3.0.0\na0: &a0 [x, x, x, x, x, x, x, x, x, x]\n");
        for i in 1..12 {
            let prev = i - 1;
            s.push_str(&format!(
                "a{i}: &a{i} [*a{prev}, *a{prev}, *a{prev}, *a{prev}, *a{prev}, *a{prev}, *a{prev}, *a{prev}, *a{prev}, *a{prev}]\n"
            ));
        }
        s.push_str("info: {title: x, version: '1'}\npaths: {}\n");
        s
    };
    let curl_quotes = format!("curl https://x -d '{}", "a".repeat(1_000_000));
    let http_lines = "GET https://x\n".repeat(200_000);
    let many_blocks = "###\n".repeat(500_000);
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("deep.json", deep_json.into_bytes()),
        ("deep.yaml", deep_yaml.into_bytes()),
        ("anchors.yaml", anchors.into_bytes()),
        ("open-quote.sh", curl_quotes.into_bytes()),
        ("many.http", http_lines.into_bytes()),
        ("blocks.http", many_blocks.into_bytes()),
    ];
    for (name, data) in &cases {
        let start = web_time::Instant::now();
        for format in formats() {
            check(name, format, name, data);
        }
        let took = start.elapsed();
        assert!(took.as_secs() < 30, "{name}: took {took:?}");
    }
}

#[test]
fn oversized_input_is_refused() {
    let data = vec![b' '; MAX_INPUT_BYTES + 1];
    let files = [SourceFile {
        name: "big.json",
        data: &data,
    }];
    assert!(matches!(
        import_files(None, &files),
        Err(ImportError::TooLarge { .. })
    ));
}

/// Multi-file OpenAPI specs (OpenAPI import wizard): one file mutated at a time, the files
/// imported together, also with the wizard's overview and every kept spec readable.
#[test]
fn mutated_multi_file_specs_never_panic() {
    let dir = corpus().join("openapi/synthetic-multi-file-3.0.bundle");
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for name in [
        "openapi.yaml",
        "parameters.yaml",
        "schemas/dog.yaml",
        "schemas/error.yaml",
    ] {
        files.push((name.to_owned(), std::fs::read(dir.join(name)).unwrap()));
    }
    let mut rng = Rng(0x5eed_0a91_77fe_0002);
    let options = rt_format::import::OpenApiOptions {
        keep_spec: true,
        ..Default::default()
    };
    for round in 0..60 {
        let mut set = files.clone();
        let i = rng.below(set.len());
        set[i].1 = mutate(&set[i].1, &mut rng);
        let sources: Vec<SourceFile<'_>> = set
            .iter()
            .map(|(name, data)| SourceFile { name, data })
            .collect();
        let label = format!("multi-file round {round} ({} mutated)", set[i].0);
        let result = std::panic::catch_unwind(|| {
            (
                import_files(None, &sources),
                rt_format::import::inspect_openapi(&sources),
                rt_format::import::import_openapi(&sources, &options),
            )
        });
        let Ok((plain, _, kept)) = result else {
            panic!("{label}: panicked");
        };
        for import in [plain, kept].into_iter().flatten() {
            let read = rt_format::read_collection(&import.files)
                .unwrap_or_else(|e| panic!("{label}: unreadable: {e}"));
            assert!(read.issues().is_empty(), "{label}: {:?}", read.issues());
        }
    }
}
