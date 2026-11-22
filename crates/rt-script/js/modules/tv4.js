// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `require('tv4')` (and Postman's global `tv4`): JSON Schema validation with the validator of
// shims/00-expect.js (`jsonSchema`); `validate`, `validateResult`, `validateMultiple`, `error`.
// `format` is not checked; `addSchema` and remote `$ref` are not included.

"use strict";

const { schemaErrors } = globalThis.__rtShim;

const tv4 = {
  error: null,
  missing: [],
  validate(data, schema) {
    const errors = schemaErrors(schema, data);
    tv4.error = errors.length ? { message: errors[0], dataPath: "", schemaPath: "" } : null;
    return errors.length === 0;
  },
  validateResult(data, schema) {
    const errors = schemaErrors(schema, data);
    return { valid: errors.length === 0, error: errors.length ? { message: errors[0] } : null, missing: [] };
  },
  validateMultiple(data, schema) {
    const errors = schemaErrors(schema, data);
    return { valid: errors.length === 0, errors: errors.map((message) => ({ message })), missing: [] };
  },
};

module.exports = tv4;
