// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt send --trace` (wave 19, strand A): prints the trace id of the send on stderr (with
//! `--json` it is in the report's `trace`). The `traceparent` header itself is set by the
//! collection's `extensions.requesttailor.trace.inject`, not by this flag. Interface: the
//! flag refuses "not implemented yet".

pub const HELP: &str = "      --trace               Print the trace id of the send on stderr\n";

/// Called by `rt send` before it starts. Strand A replaces it.
pub fn check(requested: bool) -> Result<(), String> {
    if requested {
        return Err("--trace is not implemented yet (wave 19, strand A).".into());
    }
    Ok(())
}
