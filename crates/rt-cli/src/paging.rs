// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt send --all-pages [--max-pages N] [--respect-limits] [--max-wait T]` (wave 18, strand C):
//! follows the pages of a reading request (`rt_app::paging`) and prints one report with the
//! merged body. The flags are parsed in `lib.rs` into [`PagingArgs`]; [`send_pages`] is the
//! hook `rt send` calls instead of the plain send. Interface: refuses "not implemented yet".

use rt_core::send::SendReport;
use rt_core::transport::Transport;
use rt_format::CollectionDoc;

use crate::{DirFiles, SendArgs};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PagingArgs {
    /// `--all-pages`.
    pub all_pages: bool,
    /// `--max-pages N`; the hard maximum of `rt_app::paging` applies.
    pub max_pages: Option<u32>,
}

/// Called by `rt send` for every request that is not a session. `Ok(None)`: send as usual.
/// `Ok(Some(report))`: the merged report to print. `Err(message)`: usage error (exit 2).
pub async fn send_pages<T: Transport>(
    args: &SendArgs,
    doc: &CollectionDoc,
    request_path: &str,
    files: &DirFiles,
    transport: &T,
) -> Result<Option<SendReport>, String> {
    let _ = (doc, request_path, files, transport);
    if args.paging.all_pages || args.paging.max_pages.is_some() {
        return Err("--all-pages is not implemented yet (wave 18, strand C).".into());
    }
    Ok(None)
}
