// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Importer for Postman files. Owner: wave 4, strand B.

use super::{Import, ImportError, ImportFormat};

pub(super) fn import(_file_name: &str, _data: &[u8]) -> Result<Import, ImportError> {
    super::not_yet(ImportFormat::Postman)
}
