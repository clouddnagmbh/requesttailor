// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19 interface: every new command and flag is registered, parses, and answers "not
//! implemented yet" with exit 2 until its strand lands; `--help` lists them.

use rt_cli::run;

fn rt(args: &[&str]) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(args.iter().copied().map(String::from), &mut out, &mut err);
    (
        code,
        String::from_utf8_lossy(&out).into_owned(),
        String::from_utf8_lossy(&err).into_owned(),
    )
}

#[test]
fn new_commands_are_registered_and_not_implemented_yet() {
    let cases: &[(&[&str], &str)] = &[
        (&["notebook", "run", "a.notebook.md"], "wave 19"),
        (&["send", "a.yml", "--sequence", "out.md"], "wave 19"),
    ];
    for (args, needle) in cases {
        let (code, _, err) = rt(args);
        assert_eq!(code, 2, "{args:?}: {err}");
        assert!(err.contains(needle), "{args:?}: {err}");
    }
}

#[test]
fn help_lists_the_wave_19_commands_and_the_audit_exit_code() {
    let (_, out, _) = rt(&["--help"]);
    for word in [
        "rt security audit",
        "rt notebook run",
        "--debug",
        "--trace",
        "9  rt security audit",
    ] {
        assert!(out.contains(word), "--help misses {word:?}");
    }
    assert_eq!(rt_cli::EXIT_AUDIT, 9);
    assert_eq!(rt_cli::EXIT_CONSUMERS, 8);
}
