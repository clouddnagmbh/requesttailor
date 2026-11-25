// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt send --debug [--break <line>]...` (wave 19, strand Y): the text mode of the script
//! debugger (`rt_script::debug`). The request's own scripts stop at the breakpoints (at the
//! first statement without any); at every stop the terminal shows the line and the variables
//! and reads one command from stdin:
//!
//! ```text
//! c  continue        n  step over        s  step into        o  step out
//! p <expression>     evaluate in the stopped scope           q  end the session
//! ```
//!
//! Everything goes to stderr (stdout stays the report). Without input (a closed stdin) the
//! script continues. Secret values are masked like in the report. `rt.sendRequest` is not
//! available while debugging; scripts of the collection and of folders run as usual.

use std::cell::RefCell;
use std::io::{BufRead, Write};

use rt_script::debug::{DebugCommand, DebugState, DebugStatus, DebugTerminal};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DebugArgs {
    /// `--debug`.
    pub debug: bool,
    /// `--break <line>`, repeatable.
    pub breakpoints: Vec<u32>,
}

pub const HELP: &str = "      --debug               Debug the request's scripts in the terminal (stops at --break lines)\n      --break <LINE>        A breakpoint for --debug; repeatable\n";

impl DebugArgs {
    /// Whether the scripts are debugged (`--break` alone implies `--debug`).
    pub fn active(&self) -> bool {
        self.debug || !self.breakpoints.is_empty()
    }
}

/// Called by `rt send` before it starts.
pub fn check(args: &DebugArgs) -> Result<(), String> {
    if args.breakpoints.contains(&0) {
        return Err("--break needs a line number (lines start at 1).".into());
    }
    Ok(())
}

/// The terminal of a debug session: commands from `input`, the view on `output`.
pub struct Terminal<R, W> {
    io: RefCell<(R, W)>,
}

impl<R: BufRead, W: Write> Terminal<R, W> {
    pub fn new(input: R, output: W) -> Self {
        Self {
            io: RefCell::new((input, output)),
        }
    }
}

fn describe(state: &DebugState, source: &str) -> String {
    let Some(pause) = &state.pause else {
        return String::new();
    };
    let mut text = format!(
        "\n[debug] stopped at line {} ({})\n",
        pause.line, pause.reason
    );
    let lines: Vec<&str> = source.lines().collect();
    let line = pause.line as usize;
    for n in line.saturating_sub(2).max(1)..=(line + 1).min(lines.len()) {
        let marker = if n == line { ">" } else { " " };
        text.push_str(&format!("{marker} {n:>4} | {}\n", lines[n - 1]));
    }
    if pause.frames.len() > 1 {
        let stack: Vec<String> = pause
            .frames
            .iter()
            .map(|f| format!("{} (line {})", f.function, f.line))
            .collect();
        text.push_str(&format!("  stack: {}\n", stack.join(" < ")));
    }
    for scope in &pause.scopes {
        if scope.values.is_empty() {
            continue;
        }
        text.push_str(&format!("  {}:\n", scope.name));
        for v in &scope.values {
            text.push_str(&format!("    {} = {}\n", v.name, v.preview));
        }
    }
    for line in state.output.iter().rev().take(3).rev() {
        text.push_str(&format!("  console: {line}\n"));
    }
    text
}

impl<R: BufRead, W: Write> DebugTerminal for Terminal<R, W> {
    fn paused(&self, source: &str, state: &DebugState) -> Option<DebugCommand> {
        let mut io = self.io.borrow_mut();
        let answered = state.pause.as_ref().and_then(|p| p.evaluated.as_ref());
        if let Some(value) = answered {
            let _ = writeln!(io.1, "= {} ({})", value.preview, value.kind);
        } else {
            let _ = write!(io.1, "{}", describe(state, source));
        }
        loop {
            let _ = write!(io.1, "(debug) ");
            let _ = io.1.flush();
            let mut line = String::new();
            if io.0.read_line(&mut line).unwrap_or(0) == 0 {
                let _ = writeln!(io.1);
                return None;
            }
            let line = line.trim();
            let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
            match word {
                "c" | "continue" | "" => return Some(DebugCommand::Continue),
                "n" | "next" => return Some(DebugCommand::StepOver),
                "s" | "step" => return Some(DebugCommand::StepInto),
                "o" | "out" => return Some(DebugCommand::StepOut),
                "q" | "quit" => return Some(DebugCommand::Abort),
                "p" | "print" if !rest.trim().is_empty() => {
                    return Some(DebugCommand::Evaluate {
                        expression: rest.trim().to_owned(),
                    });
                }
                _ => {
                    let _ = writeln!(
                        io.1,
                        "c continue, n step over, s step into, o step out, p <expression>, q quit"
                    );
                }
            }
        }
    }

    fn finished(&self, state: &DebugState) {
        let mut io = self.io.borrow_mut();
        match state.status {
            DebugStatus::Aborted => {
                let _ = writeln!(io.1, "[debug] session ended");
            }
            _ => {
                if let Some(e) = &state.error {
                    let _ = writeln!(io.1, "[debug] the script failed: {e}");
                } else {
                    let _ = writeln!(io.1, "[debug] script finished");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_terminal_reads_commands() {
        let state = DebugState {
            session: 1,
            status: DebugStatus::Paused,
            pause: None,
            output: Vec::new(),
            error: None,
        };
        let t = Terminal::new("what\nn\np 1 + 1\nq\n".as_bytes(), Vec::new());
        assert_eq!(t.paused("", &state), Some(DebugCommand::StepOver));
        assert_eq!(
            t.paused("", &state),
            Some(DebugCommand::Evaluate {
                expression: "1 + 1".into()
            })
        );
        assert_eq!(t.paused("", &state), Some(DebugCommand::Abort));
        assert_eq!(t.paused("", &state), None);
    }
}
