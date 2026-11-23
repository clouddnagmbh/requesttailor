// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 16, strand D: a distribution's changelog writers (`rt_cli::Distribution::
//! changelog_writers`) are chosen with their own flag of `rt changelog`, take their options as
//! `--<name> <value>`, are listed in `rt changelog --help`, and end with exit code 2 and their
//! message on an error. Without a flag `rt changelog` is byte-identical to before; the
//! golden files in `tests/golden` were written by the code of wave 15.

use std::path::{Path, PathBuf};
use std::process::Command;

use rt_app::AppError;
use rt_app::contract::{ChangelogFacts, ChangelogWriter};

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/contract")
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.test"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

/// A repository with the collection of `testdata/contract`, spec v1, then v3, then one empty
/// commit per message in `messages`. `HEAD~n..HEAD` with `n = messages.len() + 1` is the range
/// from v1 to the last commit.
fn repo(name: &str, messages: &[&str]) -> Dir {
    let root = std::env::temp_dir().join(format!("rt-writers-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let from = fixtures().join("collection");
    for entry in walk(&from) {
        let target = root.join(entry.strip_prefix(&from).unwrap());
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(&entry, &target).unwrap();
    }
    std::fs::write(
        root.join("opencollection.yml"),
        "opencollection: 1.0.0\ninfo:\n  name: Orders\n",
    )
    .unwrap();
    git(&root, &["init", "-q"]);
    let spec = |name: &str| std::fs::read(fixtures().join("specs").join(name)).unwrap();
    std::fs::write(root.join("openapi.yaml"), spec("v1.yaml")).unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "v1"]);
    std::fs::write(root.join("openapi.yaml"), spec("v3.yaml")).unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "v3"]);
    for m in messages {
        git(&root, &["commit", "-q", "--allow-empty", "-m", m]);
    }
    Dir(root)
}

/// Tells what it was given.
struct Notes;

impl ChangelogWriter for Notes {
    fn flag(&self) -> &'static str {
        "notes"
    }
    fn options(&self) -> &'static [&'static str] {
        &["audience", "lang"]
    }
    fn help(&self) -> &'static str {
        "  --notes [--audience <who>] [--lang <code>]  Release notes for people.\n"
    }
    fn write(
        &self,
        facts: &ChangelogFacts,
        options: &[(String, String)],
    ) -> Result<String, AppError> {
        let commits: Vec<String> = facts
            .commits
            .iter()
            .map(|c| format!("{}|{:?}|{}", c.summary, c.pull, c.body.replace('\n', "/")))
            .collect();
        Ok(format!(
            "notes {}..{}\noptions {options:?}\ncommits {commits:?}\nfirst {}\n",
            facts.from,
            facts.to,
            facts.markdown.lines().next().unwrap_or_default(),
        ))
    }
}

/// Always fails.
struct Broken;

impl ChangelogWriter for Broken {
    fn flag(&self) -> &'static str {
        "broken"
    }
    fn options(&self) -> &'static [&'static str] {
        &[]
    }
    fn help(&self) -> &'static str {
        "  --broken  Always fails.\n"
    }
    fn write(&self, _: &ChangelogFacts, _: &[(String, String)]) -> Result<String, AppError> {
        Err(AppError::Shell("no provider is set up".into()))
    }
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

fn rt_with(args: &[&str], writers: &[&dyn ChangelogWriter]) -> Out {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rt_cli::run_distribution(
        args.iter().copied(),
        &mut out,
        &mut err,
        &rt_cli::Distribution {
            changelog_writers: writers,
            ..Default::default()
        },
    );
    Out {
        code,
        stdout: String::from_utf8(out).unwrap(),
        stderr: String::from_utf8(err).unwrap(),
    }
}

fn path(r: &Dir) -> &str {
    r.0.to_str().unwrap()
}

#[test]
fn a_writer_flag_hands_the_facts_and_options_to_the_writer() {
    let r = repo("flag", &[]);
    let out = rt_with(
        &[
            "changelog",
            "HEAD~1..HEAD",
            path(&r),
            "--notes",
            "--audience",
            "consumers",
            "--lang",
            "de",
        ],
        &[&Notes, &Broken],
    );
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(
        out.stdout,
        "notes HEAD~1..HEAD\n".to_owned()
            + "options [(\"audience\", \"consumers\"), (\"lang\", \"de\")]\n\
               commits [\"v3|None|\"]\n\
               first # Orders API 1.0.0 → 2.0.0\n"
    );
}

#[test]
fn options_may_come_before_the_flag_and_the_out_file_still_works() {
    let r = repo("order", &[]);
    let file = r.0.join("NOTES.md");
    let out = rt_with(
        &[
            "changelog",
            "--lang",
            "en",
            "HEAD~1..HEAD",
            "--notes",
            "--out",
            file.to_str().unwrap(),
            path(&r),
        ],
        &[&Notes],
    );
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.stdout, "");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("options [(\"lang\", \"en\")]"), "{text}");
}

#[test]
fn the_commits_of_the_range_carry_their_pull_request_numbers() {
    let r = repo(
        "pulls",
        &[
            "Merge pull request #12 from acme/notes\n\nOrders get a note",
            "Add region filter (#7)",
            "Merge branch 'x' into 'main'\n\nSee merge request group/repo!31",
            "Fix typo",
        ],
    );
    let out = rt_with(
        &["changelog", "HEAD~5..HEAD", path(&r), "--notes"],
        &[&Notes],
    );
    assert_eq!(out.code, 0, "{}", out.stderr);
    // First-parent chain, newest first; the commit `v1` is the start and not part of it.
    assert!(
        out.stdout.contains(
            "commits [\"Fix typo|None|\", \
             \"Merge branch 'x' into 'main'|Some(31)|See merge request group/repo!31\", \
             \"Add region filter (#7)|Some(7)|\", \
             \"Merge pull request #12 from acme/notes|Some(12)|Orders get a note\", \
             \"v3|None|\"]"
        ),
        "{}",
        out.stdout
    );
}

#[test]
fn help_lists_every_writer_and_without_writers_nothing_is_added() {
    let with = rt_with(&["changelog", "--help"], &[&Notes, &Broken]);
    assert_eq!(with.code, 0);
    assert!(
        with.stdout.contains("rt changelog <rev>..<rev>"),
        "{}",
        with.stdout
    );
    assert!(
        with.stdout
            .contains("--notes [--audience <who>] [--lang <code>]"),
        "{}",
        with.stdout
    );
    assert!(
        with.stdout.contains("--broken  Always fails."),
        "{}",
        with.stdout
    );
    let without = rt_with(&["changelog", "--help"], &[]);
    assert!(!without.stdout.contains("--notes"), "{}", without.stdout);
    assert_eq!(
        without.stdout,
        rt_with(&["--help"], &[]).stdout,
        "the help of a build without writers is the plain help"
    );
}

#[test]
fn a_failing_writer_ends_with_exit_code_two_and_its_message() {
    let r = repo("broken", &[]);
    let out = rt_with(
        &["changelog", "HEAD~1..HEAD", path(&r), "--broken"],
        &[&Notes, &Broken],
    );
    assert_eq!(out.code, 2);
    assert_eq!(out.stdout, "");
    assert!(
        out.stderr.contains("rt changelog: no provider is set up"),
        "{}",
        out.stderr
    );
}

#[test]
fn bad_writer_arguments_are_usage_errors() {
    let r = repo("usage", &[]);
    let base = ["changelog", "HEAD~1..HEAD", path(&r)];
    let run = |extra: &[&str]| {
        let args: Vec<&str> = base.iter().chain(extra).copied().collect();
        rt_with(&args, &[&Notes, &Broken])
    };
    // An option that no writer takes.
    let out = run(&["--notes", "--nope", "x"]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("--nope"), "{}", out.stderr);
    // An option of another writer.
    let out = run(&["--broken", "--lang", "de"]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("--lang"), "{}", out.stderr);
    assert!(out.stderr.contains("--broken"), "{}", out.stderr);
    // An option without its writer.
    let out = run(&["--lang", "de"]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("--lang"), "{}", out.stderr);
    assert!(out.stderr.contains("--notes"), "{}", out.stderr);
    // A missing value.
    let out = run(&["--notes", "--lang"]);
    assert_eq!(out.code, 2);
    // Two writers.
    let out = run(&["--notes", "--broken"]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("one"), "{}", out.stderr);
    // The same option twice.
    let out = run(&["--notes", "--lang", "de", "--lang", "en"]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("--lang"), "{}", out.stderr);
}

#[test]
fn without_a_writer_the_flag_is_unknown() {
    let r = repo("unknown", &[]);
    let (none, one): (&[&dyn ChangelogWriter], &[&dyn ChangelogWriter]) = (&[], &[&Broken]);
    for writers in [none, one] {
        let out = rt_with(&["changelog", "HEAD~1..HEAD", path(&r), "--notes"], writers);
        assert_eq!(out.code, 2);
        assert_eq!(out.stdout, "");
        assert!(out.stderr.contains("--notes"), "{}", out.stderr);
    }
    // The plain `rt` builds have none either.
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rt_cli::run(
        ["changelog", "HEAD~1..HEAD", path(&r), "--notes"],
        &mut out,
        &mut err,
    );
    assert_eq!(code, 2);
}

/// Pins `rt changelog` without a writer flag: with and without writers installed, the output
/// is the bytes of `tests/golden/*.md` (written by the code before wave 16). Set
/// `RT_UPDATE_GOLDEN=1` to rewrite them on purpose.
#[test]
fn without_a_flag_the_output_is_byte_identical_to_before() {
    let r = repo("golden", &[]);
    let golden = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    for (name, args) in [
        ("changelog-breaking.md", vec!["HEAD~1..HEAD"]),
        ("changelog-same.md", vec!["HEAD..HEAD"]),
    ] {
        let mut full = vec!["changelog"];
        full.extend(&args);
        full.push(path(&r));
        let plain = rt_with(&full, &[]);
        assert_eq!(plain.code, 0, "{}", plain.stderr);
        let file = golden.join(name);
        if std::env::var_os("RT_UPDATE_GOLDEN").is_some() {
            std::fs::write(&file, &plain.stdout).unwrap();
        }
        let want = std::fs::read_to_string(&file).expect("golden file");
        assert_eq!(plain.stdout, want, "{name}");
        let with = rt_with(&full, &[&Notes, &Broken]);
        assert_eq!(with.stdout, want, "{name} with writers installed");
        assert_eq!(with.code, 0);
    }
}
