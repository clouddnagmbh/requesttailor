// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `require('uuid')`: `v4`, `NIL`, `MAX`, `validate`, `version`, `parse`, `stringify`, written
// for RequestTailor (not the uuid package). `v4` draws from Math.random: unique enough for
// test data, not for secrets. `v1`, `v3`, `v5`, `v6`, `v7` are not included.

"use strict";

const RE = /^(?:[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}|00000000-0000-0000-0000-000000000000|ffffffff-ffff-ffff-ffff-ffffffffffff)$/i;

function stringify(bytes) {
  const h = Array.from(bytes, (b) => (b < 16 ? "0" : "") + b.toString(16)).join("");
  return h.slice(0, 8) + "-" + h.slice(8, 12) + "-" + h.slice(12, 16) + "-" + h.slice(16, 20) + "-" + h.slice(20, 32);
}

function validate(s) {
  return typeof s === "string" && RE.test(s);
}

function v4(options, buf, offset) {
  const bytes = (options && (options.random || (options.rng && options.rng()))) || Array.from({ length: 16 }, () => Math.floor(Math.random() * 256));
  const b = Array.from(bytes).slice(0, 16);
  b[6] = (b[6] & 0x0f) | 0x40;
  b[8] = (b[8] & 0x3f) | 0x80;
  if (buf) {
    for (let i = 0; i < 16; i++) buf[(offset || 0) + i] = b[i];
    return buf;
  }
  return stringify(b);
}

module.exports = {
  v4,
  NIL: "00000000-0000-0000-0000-000000000000",
  MAX: "ffffffff-ffff-ffff-ffff-ffffffffffff",
  validate,
  version(s) {
    if (!validate(s)) throw new TypeError("Invalid UUID");
    return parseInt(s.slice(14, 15), 16);
  },
  parse(s) {
    if (!validate(s)) throw new TypeError("Invalid UUID");
    const h = s.replace(/-/g, "");
    return Uint8Array.from({ length: 16 }, (_x, i) => parseInt(h.substr(i * 2, 2), 16));
  },
  stringify,
};
