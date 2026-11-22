// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt docs [<collection>] --out <dir> [--env NAME]`: static HTML documentation of a
//! collection (wave 8, strand F).
//!
//! Writes `index.html` and `style.css` into `<dir>`: folders and requests (method, URL,
//! description and docs, parameters, headers, body, auth type, saved examples), light and
//! dark through `prefers-color-scheme`. The page follows the website's rules: no scripts, no
//! inline styles, nothing loaded from other servers, a `Content-Security-Policy` with
//! `default-src 'none'`.
//!
//! No secret values: secret variables have no value in the files and stay `{{name}}`; auth
//! shows its type and the variables it uses, never values; any other value the secret
//! scanner (`rt_app::secret_scan`) recognises as a credential is shown as `***`, and the
//! command says how many it masked. With `--env`, non-secret variables of that environment
//! are filled in.
//!
//! Rendering: `docs/render.rs`; masking: `docs/mask.rs`; the stylesheet: `docs/style.css`.

mod mask;
mod render;

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

pub use render::{Rendered, render};

/// The stylesheet written next to `index.html`.
pub const STYLE: &str = include_str!("docs/style.css");

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Args {
    /// A folder of the collection (default: the current folder).
    pub collection: PathBuf,
    pub out: Option<PathBuf>,
    pub environment: Option<String>,
    pub help: bool,
}

pub fn help() -> String {
    format!(
        "\
rt docs: write static HTML documentation of a collection

Usage:
  rt docs [<collection>] --out <dir> [options]

Writes <dir>/index.html and <dir>/style.css: every folder and request with
method, URL, description and docs, parameters, headers, body, auth type and the
saved examples. The page runs no scripts and loads nothing from other servers;
light and dark follow the system.

Secret values never appear: secret variables stay {{{{name}}}}, auth shows only its
type and variables, and values that look like credentials are shown as ***.

<collection> is the collection folder or any folder inside it (default: the
current folder).

Options:
  -o, --out <DIR>       Folder to write to (created if missing; index.html and
                        style.css in it are replaced)
  -e, --env <NAME>      Fill in the non-secret variables of environment NAME
  -h, --help            Show this help

Exit code {ok} when written, {usage} when the collection cannot be read or the
folder cannot be written.
",
        ok = crate::EXIT_OK,
        usage = crate::EXIT_USAGE,
    )
}

/// Parses the arguments after `rt docs`. `--help` is a flag ([`Args::help`]), so `rt docs
/// --help` prints this command's help.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;

    let err = |e: lexopt::Error| format!("docs: {e}");
    let mut args = Args {
        collection: PathBuf::from("."),
        ..Args::default()
    };
    let mut collection: Option<OsString> = None;
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => args.help = true,
            Short('o') | Long("out") => args.out = Some(parser.value().map_err(err)?.into()),
            Short('e') | Long("env") => {
                args.environment = Some(parser.value().map_err(err)?.string().map_err(err)?)
            }
            Value(v) if collection.is_none() => collection = Some(v),
            other => return Err(format!("docs: {}", other.unexpected())),
        }
    }
    if let Some(c) = collection {
        args.collection = c.into();
    }
    if args.out.is_none() && !args.help {
        return Err("docs: missing --out <dir>".into());
    }
    Ok(Some(args))
}

pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    if args.help {
        let _ = stdout.write_all(help().as_bytes());
        return crate::EXIT_OK;
    }
    let Some(out) = args.out else {
        let _ = writeln!(stderr, "rt docs: missing --out <dir>");
        return crate::EXIT_USAGE;
    };
    let rendered = crate::mock::collection_root(&args.collection)
        .and_then(|root| crate::load_collection(&root))
        .and_then(|doc| render(&doc, args.environment.as_deref()));
    let rendered = match rendered {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(stderr, "rt docs: {e}");
            return crate::EXIT_USAGE;
        }
    };
    let written = std::fs::create_dir_all(&out)
        .and_then(|()| std::fs::write(out.join("index.html"), &rendered.html))
        .and_then(|()| std::fs::write(out.join("style.css"), STYLE));
    if let Err(e) = written {
        let _ = writeln!(stderr, "rt docs: cannot write {}: {e}", out.display());
        return crate::EXIT_USAGE;
    }
    if rendered.masked > 0 {
        let _ = writeln!(
            stderr,
            "rt docs: {} value{} that look like credentials shown as *** (run `rt scan` to find them)",
            rendered.masked,
            if rendered.masked == 1 { "" } else { "s" }
        );
    }
    let _ = writeln!(
        stdout,
        "rt docs: wrote {} ({} request{})",
        out.join("index.html").display(),
        rendered.requests,
        if rendered.requests == 1 { "" } else { "s" }
    );
    crate::EXIT_OK
}

#[cfg(test)]
mod tests;
