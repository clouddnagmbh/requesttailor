// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! RequestTailor's commit scanner (seam N2): `rt_app::secret_scan`, which knows the fields of
//! request files (auth, headers, `secret: true` variables) as well as token formats. Files
//! the scanner skips (`secret_scan::text_of`: not UTF-8, NUL bytes, over 4 MiB) pass.

use rt_app::secret_scan;
use tk_git_types::CommitScanner;

/// Refuses commits with credentials typed into files (`# rt-scan:allow` lets a line pass).
#[derive(Debug, Clone, Copy, Default)]
pub struct SecretScanner;

impl CommitScanner for SecretScanner {
    fn refusal(&self, files: &[(&str, &[u8])]) -> Option<String> {
        let texts: Vec<(&str, &str)> = files
            .iter()
            .filter_map(|(path, bytes)| Some((*path, secret_scan::text_of(bytes)?)))
            .collect();
        let findings = secret_scan::scan(&texts);
        (!findings.is_empty()).then(|| secret_scan::refusal(&findings))
    }
}
