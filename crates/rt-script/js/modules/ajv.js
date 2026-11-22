// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `require('ajv')`: `new Ajv().compile(schema)` and `ajv.validate(schema, data)` with the
// JSON Schema validator of shims/00-expect.js. Errors are `{ message }` objects; `format`,
// custom keywords and remote `$ref` are not included.

"use strict";

const { schemaErrors } = globalThis.__rtShim;

function Ajv() {
  if (!(this instanceof Ajv)) return new Ajv();
  this.errors = null;
}

Ajv.prototype.compile = function (schema) {
  const validate = (data) => {
    const errors = schemaErrors(schema, data);
    validate.errors = errors.length ? errors.map((message) => ({ message })) : null;
    return errors.length === 0;
  };
  validate.errors = null;
  return validate;
};

Ajv.prototype.validate = function (schema, data) {
  const errors = schemaErrors(schema, data);
  this.errors = errors.length ? errors.map((message) => ({ message })) : null;
  return errors.length === 0;
};

Ajv.prototype.errorsText = function (errors) {
  const list = errors === undefined ? this.errors : errors;
  return list && list.length ? list.map((e) => e.message).join(", ") : "No errors";
};

Ajv.prototype.addFormat = function () {
  return this;
};

Ajv.default = Ajv;
module.exports = Ajv;
