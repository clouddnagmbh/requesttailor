// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt send --view <name>` and `--view-expr <expression>` (wave 18, strand G): prints what a
//! saved view (`<request>.views.json`) or an expression picks from the response instead of
//! the whole report, masked. The flags are parsed in `lib.rs` into [`ViewArgs`]; [`render`]
//! is the hook `rt send` calls. Interface: refuses "not implemented yet".

use rt_core::send::SendReport;

use crate::SendArgs;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ViewArgs {
    /// `--view <name>`: a saved view of the request.
    pub name: Option<String>,
    /// `--view-expr <expression>`: JSONPath, or jq where the build has it.
    pub expression: Option<String>,
}

/// Called by `rt send` on the finished report. `Ok(None)`: print the report as usual.
/// `Ok(Some(text))`: print this instead. `Err(message)`: usage error (exit 2).
pub fn render(args: &SendArgs, report: &SendReport) -> Result<Option<String>, String> {
    let _ = report;
    if args.view.name.is_some() || args.view.expression.is_some() {
        return Err("--view is not implemented yet (wave 18, strand G).".into());
    }
    Ok(None)
}
