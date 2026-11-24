// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `--sequence <file> [--sequence-values]` of `rt send`, `rt run` and `rt mcp` (wave 19,
//! strand G): writes a Mermaid sequence diagram of what was sent, as a Markdown block, when
//! the command ends (and, for `rt mcp`, every 5 s), atomically. Values only with
//! `--sequence-values`. The flags are parsed into [`SequenceArgs`]; the commands call
//! [`check`] first. Interface: any use refuses "not implemented yet".

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SequenceArgs {
    /// `--sequence FILE`.
    pub out: Option<PathBuf>,
    /// `--sequence-values`: short masked excerpts of values (may contain personal data).
    pub values: bool,
}

pub const HELP: &str = "      --sequence <FILE>     Write a Mermaid sequence diagram of the run to FILE (Markdown)\n      --sequence-values     Include short, masked excerpts of values (may hold personal data)\n";

/// The one place the flags are read; `name` is `sequence` or `sequence-values`.
pub fn parse_option(
    name: &str,
    parser: &mut lexopt::Parser,
    args: &mut SequenceArgs,
) -> Result<(), String> {
    let err = |e: lexopt::Error| e.to_string();
    match name {
        "sequence" => args.out = Some(parser.value().map_err(err)?.into()),
        _ => args.values = true,
    }
    Ok(())
}

/// Called by the commands before they start. Strand G replaces it with the recorder setup.
pub fn check(args: &SequenceArgs) -> Result<(), String> {
    if args.out.is_some() || args.values {
        return Err("--sequence is not implemented yet (wave 19, strand G).".into());
    }
    Ok(())
}
