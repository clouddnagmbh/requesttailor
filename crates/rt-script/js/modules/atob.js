// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `require('atob')`: the global `atob` (Base64 → Latin-1 text), as the npm package exports it.

"use strict";

module.exports = globalThis.__rtShim.atob;
