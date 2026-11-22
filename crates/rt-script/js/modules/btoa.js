// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `require('btoa')`: the global `btoa` (Latin-1 text → Base64), as the npm package exports it.

"use strict";

module.exports = globalThis.__rtShim.btoa;
