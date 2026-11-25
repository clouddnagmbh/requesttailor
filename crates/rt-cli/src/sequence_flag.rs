// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `--sequence <file> [--sequence-values]` of `rt send`, `rt run` and `rt mcp` (wave 19,
//! strand G): writes a Mermaid sequence diagram of what was sent, as a Markdown block, when
//! the command ends (and, for `rt mcp`, every 5 s), atomically: the text goes to a temporary
//! file next to the target and is renamed over it, so a reader never sees half a diagram.
//! Values only with `--sequence-values`.
//!
//! The flags are parsed into [`SequenceArgs`]. [`attach`] adds a `rt_app::sequence::Recorder`
//! to the command's exchange observers (the same `send_with` path every send uses, so
//! the diagram shows what really went out, masked); [`write`] renders and writes it.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rt_app::sequence::{Recorder, SequenceOptions, SequenceSource};
use rt_core::observe::ObserverSet;

/// The recorder of one command; compares as equal so argument structs stay comparable.
#[derive(Clone, Default)]
pub struct RecorderHandle(pub Option<Arc<Recorder>>);

impl fmt::Debug for RecorderHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_some() { "Recorder" } else { "None" })
    }
}

impl PartialEq for RecorderHandle {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for RecorderHandle {}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SequenceArgs {
    /// `--sequence FILE`.
    pub out: Option<PathBuf>,
    /// `--sequence-values`: short masked excerpts of values (may contain personal data).
    pub values: bool,
    /// Set by [`attach`].
    pub recorder: RecorderHandle,
}

pub const HELP: &str = "      --sequence <FILE>     Write a Mermaid sequence diagram of the run to FILE (Markdown)\n      --sequence-values     Include short, masked excerpts of values (may hold personal data)\n";

/// How often `rt mcp` rewrites the diagram while it runs.
pub const MCP_INTERVAL: Duration = Duration::from_secs(5);

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

/// Called by the commands before they start: the flags must make sense.
pub fn check(args: &SequenceArgs) -> Result<(), String> {
    if args.values && args.out.is_none() {
        return Err("--sequence-values needs --sequence <FILE>.".into());
    }
    if let Some(out) = &args.out
        && out.is_dir()
    {
        return Err(format!(
            "--sequence {}: that is a folder; name a file such as run.md.",
            out.display()
        ));
    }
    Ok(())
}

/// With `--sequence`, adds a recorder to `observers` and keeps it in `args`.
pub fn attach(args: &mut SequenceArgs, observers: &mut ObserverSet) {
    if args.out.is_none() {
        return;
    }
    let recorder = Arc::new(Recorder::new());
    *observers = observers.clone().with(recorder.clone());
    args.recorder = RecorderHandle(Some(recorder));
}

/// Writes the diagram of everything recorded; nothing happens without `--sequence`.
pub fn write(args: &SequenceArgs) -> Result<(), String> {
    let (Some(out), Some(recorder)) = (&args.out, &args.recorder.0) else {
        return Ok(());
    };
    write_to(out, recorder, args.values)
}

fn write_to(out: &Path, recorder: &Recorder, values: bool) -> Result<(), String> {
    let options = SequenceOptions {
        include_query: false,
        include_values: values,
    };
    let model = recorder.model(&SequenceSource::Session, &options)?;
    let text = rt_app::sequence::render(&model, &options);
    let name = out
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "sequence.md".into());
    let tmp = out.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, text.markdown.as_bytes())
        .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, out).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("cannot write {}: {e}", out.display())
    })
}

/// Writes the file; a failure is said on `stderr` and turns a success into a usage error.
pub fn finish(args: &SequenceArgs, stderr: &mut dyn std::io::Write, code: i32) -> i32 {
    // A command that failed before sending anything leaves an existing file alone.
    if code != crate::EXIT_OK && args.recorder.0.as_ref().is_none_or(|r| r.is_empty()) {
        return code;
    }
    match write(args) {
        Ok(()) => code,
        Err(e) => {
            let _ = writeln!(stderr, "rt: {e}");
            if code == crate::EXIT_OK {
                crate::EXIT_USAGE
            } else {
                code
            }
        }
    }
}

/// Rewrites the file every [`MCP_INTERVAL`] until dropped (then once more).
pub struct Periodic {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Periodic {
    pub fn start(args: &SequenceArgs) -> Option<Self> {
        let (Some(out), Some(recorder)) = (args.out.clone(), args.recorder.0.clone()) else {
            return None;
        };
        let values = args.values;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let handle = std::thread::spawn(move || {
            let mut waited = Duration::ZERO;
            let step = Duration::from_millis(100);
            while !flag.load(Ordering::SeqCst) {
                std::thread::sleep(step);
                waited += step;
                if waited >= MCP_INTERVAL {
                    waited = Duration::ZERO;
                    let _ = write_to(&out, &recorder, values);
                }
            }
        });
        Some(Self {
            stop,
            handle: Some(handle),
        })
    }

    /// Stops the writer thread.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}
