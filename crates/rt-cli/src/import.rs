// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt import <file>... [--format F] --out <dir> [--json]`: converts an export of another
//! tool (`rt_format::import`) into a new collection folder and prints the loss report.
//! Owner: wave 4, strand B.
//!
//! Several files form one collection: a Postman collection with its environment and globals
//! files, `.http` files with `http-client.env.json`, several cURL files, a multi-file OpenAPI
//! spec. `--out` must not exist or be an empty folder. Exit code 0 when the collection was
//! written, even with losses.
//!
//! `rt import openapi <file|url>... --out <dir> [--names operation-id] [--group tag|path|flat]
//! [--keep-spec] [--json]` is the OpenAPI import wizard's choices on the command line
//! (`rt_format::import::import_openapi`): several files are one multi-file spec (named by
//! their paths relative to the folder they share, as the `$ref`s name them), a `.zip` holds
//! one, a URL is fetched with the files its relative `$ref`s name
//! (`rt_app::openapi::fetch_spec`, over the transport `rt send` uses).

use std::io::Write;
use std::path::{Component, Path, PathBuf};

use rt_format::import::{
    Import, ImportFormat, LossEffect, LossKind, MAX_INPUT_BYTES, OpenApiGrouping, OpenApiNaming,
    OpenApiOptions, SourceFile, import_files, import_openapi,
};

use crate::{EXIT_OK, EXIT_TRANSPORT, EXIT_USAGE};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportArgs {
    pub files: Vec<PathBuf>,
    pub format: Option<ImportFormat>,
    pub out: PathBuf,
    pub json: bool,
    /// `rt import openapi …`: the wizard's choices (`files` may hold one URL).
    pub openapi: Option<OpenApiOptions>,
}

fn is_url(arg: &Path) -> bool {
    let s = arg.to_string_lossy();
    s.starts_with("http://") || s.starts_with("https://")
}

pub(crate) fn parse(parser: &mut lexopt::Parser) -> Result<Option<ImportArgs>, String> {
    use lexopt::prelude::*;

    let err = |e: lexopt::Error| e.to_string();
    let mut files = Vec::new();
    let mut format = None;
    let mut out = None;
    let mut json = false;
    let mut openapi: Option<OpenApiOptions> = None;
    let mut wizard_flag = None;
    let mut wizard = false;
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Short('f') | Long("format") => {
                let name = parser.value().map_err(err)?.string().map_err(err)?;
                format = Some(parse_format(&name)?);
            }
            Short('o') | Long("out") => out = Some(PathBuf::from(parser.value().map_err(err)?)),
            Long("json") => json = true,
            Long("names") => {
                let v = parser.value().map_err(err)?.string().map_err(err)?;
                let naming = match v.as_str() {
                    "summary" => OpenApiNaming::Summary,
                    "operation-id" => OpenApiNaming::OperationId,
                    _ => return Err(format!("unknown --names {v:?}; summary or operation-id")),
                };
                openapi.get_or_insert_default().naming = naming;
                wizard_flag.get_or_insert("--names");
            }
            Long("group") => {
                let v = parser.value().map_err(err)?.string().map_err(err)?;
                let grouping = match v.as_str() {
                    "tag" => OpenApiGrouping::Tag,
                    "path" => OpenApiGrouping::Path,
                    "flat" => OpenApiGrouping::Flat,
                    _ => return Err(format!("unknown --group {v:?}; tag, path or flat")),
                };
                openapi.get_or_insert_default().grouping = grouping;
                wizard_flag.get_or_insert("--group");
            }
            Long("keep-spec") => {
                openapi.get_or_insert_default().keep_spec = true;
                wizard_flag.get_or_insert("--keep-spec");
            }
            Value(v) if files.is_empty() && !wizard && v == "openapi" => wizard = true,
            Value(v) => files.push(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    if let Some(flag) = wizard_flag.filter(|_| !wizard) {
        return Err(format!("import: {flag} needs `rt import openapi`"));
    }
    if wizard && format.is_some() {
        return Err("import openapi: --format does not apply".into());
    }
    if files.is_empty() {
        return Err(if wizard {
            "import openapi: missing <file> or <url>".into()
        } else {
            "import: missing <file>".into()
        });
    }
    if files.iter().any(|f| is_url(f)) && (!wizard || files.len() > 1) {
        return Err(if wizard {
            "import openapi: give one URL, or files".into()
        } else {
            "import: URLs are imported with `rt import openapi <url>`".into()
        });
    }
    let out = out.ok_or("import: missing --out <dir>")?;
    Ok(Some(ImportArgs {
        files,
        format,
        out,
        json,
        openapi: wizard.then(|| openapi.unwrap_or_default()),
    }))
}

fn parse_format(name: &str) -> Result<ImportFormat, String> {
    serde_json::from_value(serde_json::Value::String(name.to_ascii_lowercase())).map_err(|_| {
        let names: Vec<String> = ImportFormat::ALL
            .iter()
            .map(|f| {
                serde_json::to_value(f)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default()
            })
            .collect();
        format!("unknown format {name:?}; one of {}", names.join(", "))
    })
}

/// Refuses an `--out` that exists and is not an empty folder.
fn check_out(out: &Path) -> Result<(), String> {
    match std::fs::read_dir(out) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                return Err(format!("{} already exists and is not empty", out.display()));
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) if out.exists() => Err(format!("{} exists and is not a folder", out.display())),
        Err(e) => Err(format!("cannot use {}: {e}", out.display())),
    }
}

/// Writes the collection's files below `out`.
fn write(out: &Path, import: &Import) -> Result<(), String> {
    for (path, bytes) in &import.files.files {
        let rel = Path::new(path);
        // The importer writes plain relative paths; anything else would leave `out`.
        if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
            return Err(format!("refusing to write unexpected path {path:?}"));
        }
        let target = out.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        std::fs::write(&target, bytes)
            .map_err(|e| format!("cannot write {}: {e}", target.display()))?;
    }
    Ok(())
}

/// The readable report: what was imported, then the losses grouped by kind.
pub fn render_text(import: &Import, out: &Path) -> String {
    let r = &import.report;
    let plural = |n: u32, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let mut s = format!(
        "Imported {} \"{}\" into {}\n  {}, {}, {}, {}\n",
        r.format,
        import.collection_name,
        out.display(),
        plural(r.requests, "request", "requests"),
        plural(r.folders, "folder", "folders"),
        plural(r.environments, "environment", "environments"),
        plural(import.files.files.len() as u32, "file", "files"),
    );
    if r.losses.is_empty() {
        s.push_str("\nNothing was lost: everything was imported exactly.\n");
        return s;
    }
    let dropped = r
        .losses
        .iter()
        .filter(|l| l.effect == LossEffect::Dropped)
        .count();
    s.push_str(&format!(
        "\nLoss report: {} ({dropped} dropped, {} approximated)\n",
        r.losses.len(),
        r.losses.len() - dropped
    ));
    for kind in LossKind::ALL {
        let group: Vec<_> = r.losses.iter().filter(|l| l.kind == kind).collect();
        if group.is_empty() {
            continue;
        }
        s.push_str(&format!("\n{} ({})\n", kind.label(), group.len()));
        for l in group {
            let effect = match l.effect {
                LossEffect::Dropped => "dropped     ",
                LossEffect::Approximated => "approximated",
            };
            s.push_str(&format!("  {effect}  {}: {}\n", l.location, l.detail));
        }
    }
    s
}

pub fn render_json(import: &Import, out: &Path) -> String {
    let value = serde_json::json!({
        "collectionName": import.collection_name,
        "out": out.display().to_string(),
        "files": import.files.files.keys().collect::<Vec<_>>(),
        "report": import.report,
    });
    let mut s = serde_json::to_string_pretty(&value).expect("report serializes");
    s.push('\n');
    s
}

pub(crate) fn run(args: ImportArgs, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let fail = |stderr: &mut dyn Write, msg: &str| {
        let _ = writeln!(stderr, "rt import: {msg}");
        EXIT_USAGE
    };
    if let Some(options) = &args.openapi
        && let [url] = args.files.as_slice()
        && is_url(url)
    {
        if let Err(e) = check_out(&args.out) {
            return fail(stderr, &e);
        }
        let fetched = match fetch(&url.to_string_lossy()) {
            Ok(f) => f,
            Err(e) => {
                let _ = writeln!(stderr, "rt import: {e}");
                return EXIT_TRANSPORT;
            }
        };
        let sources: Vec<SourceFile<'_>> = fetched
            .files
            .iter()
            .map(|f| SourceFile {
                name: &f.name,
                data: f.text.as_bytes(),
            })
            .collect();
        let mut import = match import_openapi(&sources, options) {
            Ok(i) => i,
            Err(e) => return fail(stderr, &e.to_string()),
        };
        // What could not be fetched belongs to the report, with the collection's losses.
        let losses = &mut import.report.losses;
        losses.extend(fetched.losses);
        losses.sort_by(|a, b| {
            let rank = |l: &str| u8::from(l != "collection");
            rank(&a.location)
                .cmp(&rank(&b.location))
                .then_with(|| a.location.cmp(&b.location))
                .then(a.kind.cmp(&b.kind))
        });
        losses.dedup();
        return finish(&args, &import, stdout, stderr);
    }
    let mut data = Vec::with_capacity(args.files.len());
    let mut total = 0usize;
    for path in &args.files {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => return fail(stderr, &format!("cannot read {}: {e}", path.display())),
        };
        total = total.saturating_add(bytes.len());
        if total > MAX_INPUT_BYTES {
            return fail(
                stderr,
                &format!(
                    "the files are larger than {} MB together",
                    MAX_INPUT_BYTES / (1024 * 1024)
                ),
            );
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        data.push((name, bytes));
    }
    if args.openapi.is_some() {
        // A multi-file spec: the files' paths relative to the folder they share.
        for ((name, _), relative) in data.iter_mut().zip(relative_names(&args.files)) {
            *name = relative;
        }
    }
    if let Err(e) = check_out(&args.out) {
        return fail(stderr, &e);
    }
    let sources: Vec<SourceFile<'_>> = data
        .iter()
        .map(|(name, bytes)| SourceFile { name, data: bytes })
        .collect();
    let result = match &args.openapi {
        Some(options) => import_openapi(&sources, options),
        None => import_files(args.format, &sources),
    };
    let import = match result {
        Ok(i) => i,
        Err(e) => return fail(stderr, &e.to_string()),
    };
    finish(&args, &import, stdout, stderr)
}

/// Fetches a spec from a URL over the native transport.
fn fetch(url: &str) -> Result<rt_app::openapi::FetchedSpec, String> {
    let transport =
        rt_core::transport::native::NativeTransport::new().map_err(|e| e.to_string())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("cannot start runtime: {e}"))?;
    runtime
        .block_on(rt_app::openapi::fetch_spec(&transport, url))
        .map_err(|e| match e {
            rt_app::AppError::Import(m) => m,
            other => other.to_string(),
        })
}

/// Each path relative to the deepest folder all of them are in, `/`-separated.
fn relative_names(paths: &[PathBuf]) -> Vec<String> {
    let absolute: Vec<PathBuf> = paths
        .iter()
        .map(|p| std::path::absolute(p).unwrap_or_else(|_| p.clone()))
        .collect();
    let dirs: Vec<Vec<Component<'_>>> = absolute
        .iter()
        .map(|p| {
            p.parent()
                .map(|d| d.components().collect())
                .unwrap_or_default()
        })
        .collect();
    let mut common = dirs.first().cloned().unwrap_or_default();
    for d in &dirs[1.min(dirs.len())..] {
        let n = common.iter().zip(d).take_while(|(a, b)| a == b).count();
        common.truncate(n);
    }
    absolute
        .iter()
        .map(|p| {
            p.components()
                .skip(common.len())
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect()
}

/// Writes the collection and prints the report.
fn finish(
    args: &ImportArgs,
    import: &Import,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    if let Err(e) = write(&args.out, import) {
        let _ = writeln!(stderr, "rt import: {e}");
        return EXIT_USAGE;
    }
    let text = if args.json {
        render_json(import, &args.out)
    } else {
        render_text(import, &args.out)
    };
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.flush();
    EXIT_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_import(args: &[&str]) -> Result<Option<ImportArgs>, String> {
        let mut parser = lexopt::Parser::from_args(args.iter().copied());
        parse(&mut parser)
    }

    #[test]
    fn arguments() {
        let a = parse_import(&[
            "a.json", "b.json", "--format", "Postman", "-o", "out", "--json",
        ])
        .unwrap()
        .unwrap();
        assert_eq!(a.files, [PathBuf::from("a.json"), PathBuf::from("b.json")]);
        assert_eq!(a.format, Some(ImportFormat::Postman));
        assert_eq!(a.out, PathBuf::from("out"));
        assert!(a.json);
        assert_eq!(parse_import(&["--help"]).unwrap(), None);
        for bad in [
            &["--out", "o"][..],
            &["a.json"],
            &["a", "--out", "o", "--format", "nope"],
            &["a", "--bogus"],
        ] {
            assert!(parse_import(bad).is_err(), "{bad:?}");
        }
        let e = parse_import(&["a", "--out", "o", "-f", "nope"]).unwrap_err();
        assert!(e.contains("postman, insomnia"), "{e}");
        assert_eq!(a.openapi, None);
    }

    #[test]
    fn openapi_arguments() {
        let a = parse_import(&[
            "openapi",
            "api/openapi.yaml",
            "api/schemas/pet.yaml",
            "--names",
            "operation-id",
            "--group",
            "path",
            "--keep-spec",
            "-o",
            "out",
        ])
        .unwrap()
        .unwrap();
        assert_eq!(
            a.files,
            [
                PathBuf::from("api/openapi.yaml"),
                PathBuf::from("api/schemas/pet.yaml")
            ]
        );
        let o = a.openapi.unwrap();
        assert_eq!(
            (o.naming, o.grouping, o.keep_spec),
            (OpenApiNaming::OperationId, OpenApiGrouping::Path, true)
        );
        let url = parse_import(&["openapi", "https://x.example.com/a.yaml", "-o", "o"])
            .unwrap()
            .unwrap();
        assert_eq!(url.openapi, Some(OpenApiOptions::default()));
        // A file named `openapi` after the first file is a file.
        let f = parse_import(&["a.yaml", "openapi", "-o", "o"])
            .unwrap()
            .unwrap();
        assert_eq!(f.files.len(), 2);
        for bad in [
            &["openapi", "-o", "o"][..],
            &["a.yaml", "--keep-spec", "-o", "o"],
            &["openapi", "a.yaml", "--group", "tags", "-o", "o"],
            &["openapi", "a.yaml", "--names", "id", "-o", "o"],
            &["openapi", "a.yaml", "-f", "postman", "-o", "o"],
            &["openapi", "https://a", "https://b", "-o", "o"],
            &["https://a.example.com/x.yaml", "-o", "o"],
        ] {
            assert!(parse_import(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn names_relative_to_the_shared_folder() {
        let paths = [
            PathBuf::from("/x/api/openapi.yaml"),
            PathBuf::from("/x/api/schemas/pet.yaml"),
            PathBuf::from("/x/api/parameters.yaml"),
        ];
        assert_eq!(
            relative_names(&paths),
            ["openapi.yaml", "schemas/pet.yaml", "parameters.yaml"]
        );
        assert_eq!(
            relative_names(&[PathBuf::from("/x/api/openapi.yaml")]),
            ["openapi.yaml"]
        );
        assert_eq!(
            relative_names(&[
                PathBuf::from("/x/spec/main.yaml"),
                PathBuf::from("/x/common/a.yaml")
            ]),
            ["spec/main.yaml", "common/a.yaml"]
        );
    }
}
