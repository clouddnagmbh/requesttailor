// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The run wizard's form, generated from a JSON Schema (an `InputField.schema` from the core,
// local `$ref`s already inlined): what control a value gets, its validation message, a value
// to "generate", and the body as a form for flat objects. Pure functions, unit-tested in
// form.test.ts. The core checks the same rules before sending (`rt_format::contract`); the
// form checks as the user types so the messages appear before "Fill and send".

export type Schema = Record<string, unknown>;

export type FieldKind =
  | "text"
  | "enum"
  | "boolean"
  | "integer"
  | "number"
  | "date"
  | "date-time"
  | "time"
  | "uuid"
  | "email"
  | "uri"
  | "password"
  | "json";

export interface FieldModel {
  kind: FieldKind;
  /** Enum values as text (kind `enum`; `boolean` has `true`/`false`). */
  options: string[];
  /** Short hint under the field: constraints in plain words ("1 to 100", "pattern ^[a-z]+$"). */
  constraints: string | null;
  placeholder: string;
  /** "Generate" makes sense (uuid, date, date-time, time, email). */
  generatable: boolean;
}

export const asSchema = (s: unknown): Schema => (s && typeof s === "object" && !Array.isArray(s) ? (s as Schema) : {});

/** The schema with `allOf` merged (shallow, as the core's `effective`). */
export function effective(input: unknown): Schema {
  const s = asSchema(input);
  const all = Array.isArray(s.allOf) ? s.allOf : [];
  if (all.length === 0) return s;
  const out: Schema = { ...s };
  delete out.allOf;
  const props: Schema = { ...asSchema(s.properties) };
  const required = new Set<string>(Array.isArray(s.required) ? (s.required as string[]) : []);
  for (const part of all) {
    const p = effective(part);
    for (const [k, v] of Object.entries(p)) {
      if (k === "properties") Object.assign(props, asSchema(v));
      else if (k === "required" && Array.isArray(v)) v.forEach((r) => required.add(String(r)));
      else if (!(k in out)) out[k] = v;
    }
  }
  if (Object.keys(props).length) out.properties = props;
  if (required.size) out.required = [...required];
  return out;
}

/** The first non-null type (`["string", "null"]` → `string`), or one implied by the keywords. */
export function typeOf(input: unknown): string {
  const s = effective(input);
  const t = s.type;
  if (typeof t === "string") return t;
  if (Array.isArray(t)) return (t.find((x) => x !== "null") as string | undefined) ?? "null";
  if (s.properties) return "object";
  if (s.items) return "array";
  if (Array.isArray(s.enum)) return typeof s.enum[0] === "number" ? "number" : "string";
  return "";
}

const num = (v: unknown): number | null => (typeof v === "number" && Number.isFinite(v) ? v : null);

function lowerBound(s: Schema): { value: number; exclusive: boolean } | null {
  const ex = s.exclusiveMinimum;
  if (num(ex) !== null) return { value: ex as number, exclusive: true };
  const min = num(s.minimum);
  return min === null ? null : { value: min, exclusive: ex === true };
}

function upperBound(s: Schema): { value: number; exclusive: boolean } | null {
  const ex = s.exclusiveMaximum;
  if (num(ex) !== null) return { value: ex as number, exclusive: true };
  const max = num(s.maximum);
  return max === null ? null : { value: max, exclusive: ex === true };
}

function constraintText(s: Schema, type: string): string | null {
  const parts: string[] = [];
  if (type === "integer" || type === "number") {
    const lo = lowerBound(s);
    const hi = upperBound(s);
    if (lo && hi && !lo.exclusive && !hi.exclusive) parts.push(`${lo.value} to ${hi.value}`);
    else {
      if (lo) parts.push(lo.exclusive ? `more than ${lo.value}` : `at least ${lo.value}`);
      if (hi) parts.push(hi.exclusive ? `less than ${hi.value}` : `at most ${hi.value}`);
    }
    if (num(s.multipleOf) !== null) parts.push(`multiple of ${s.multipleOf}`);
  }
  if (type === "string") {
    const min = num(s.minLength);
    const max = num(s.maxLength);
    if (min !== null && max !== null) parts.push(min === max ? `${min} characters` : `${min} to ${max} characters`);
    else if (min !== null) parts.push(`at least ${min} ${min === 1 ? "character" : "characters"}`);
    else if (max !== null) parts.push(`at most ${max} characters`);
    if (typeof s.pattern === "string") parts.push(`pattern ${s.pattern}`);
  }
  return parts.length ? parts.join(", ") : null;
}

const FORMAT_KINDS: Record<string, FieldKind> = {
  date: "date",
  "date-time": "date-time",
  time: "time",
  uuid: "uuid",
  email: "email",
  uri: "uri",
  url: "uri",
  password: "password",
};

const PLACEHOLDERS: Partial<Record<FieldKind, string>> = {
  uuid: "00000000-0000-0000-0000-000000000000",
  email: "name@example.com",
  uri: "https://",
  date: "YYYY-MM-DD",
  "date-time": "YYYY-MM-DDThh:mm:ssZ",
  time: "hh:mm:ss",
  json: "{ }",
};

/** What control the value gets. */
export function describe(schema: unknown): FieldModel {
  const s = effective(schema);
  const type = typeOf(s);
  const values = Array.isArray(s.enum) ? s.enum.filter((v) => v !== null).map((v) => String(v)) : [];
  let kind: FieldKind = "text";
  if (values.length > 0) kind = "enum";
  else if (type === "boolean") kind = "boolean";
  else if (type === "integer") kind = "integer";
  else if (type === "number") kind = "number";
  else if (type === "object" || type === "array") kind = "json";
  else if (typeof s.format === "string" && FORMAT_KINDS[s.format]) kind = FORMAT_KINDS[s.format]!;
  return {
    kind,
    options: kind === "boolean" ? ["true", "false"] : values,
    constraints: constraintText(s, type),
    placeholder: PLACEHOLDERS[kind] ?? (s.example !== undefined ? String(s.example) : ""),
    generatable: ["uuid", "date", "date-time", "time", "email"].includes(kind),
  };
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
const DATE = /^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])$/;
const TIME = /^([01]\d|2[0-3]):[0-5]\d:[0-5]\d(\.\d+)?(Z|[+-]\d{2}:\d{2})?$/i;
const DATE_TIME = /^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])[Tt ]([01]\d|2[0-3]):[0-5]\d:[0-5]\d(\.\d+)?(Z|[+-]\d{2}:\d{2})$/i;

function formatProblem(format: unknown, text: string): string | null {
  switch (format) {
    case "uuid":
      return UUID.test(text) ? null : "must be a UUID";
    case "email":
      return EMAIL.test(text) ? null : "must be an email address";
    case "date":
      return DATE.test(text) ? null : "must be a date (YYYY-MM-DD)";
    case "date-time":
      return DATE_TIME.test(text) ? null : "must be a date and time (RFC 3339)";
    case "time":
      return TIME.test(text) ? null : "must be a time (hh:mm:ss)";
    case "uri":
    case "url":
      try {
        new URL(text);
        return null;
      } catch {
        return "must be a full URL";
      }
    default:
      return null;
  }
}

const isTemplate = (text: string) => /\{\{[^}]+\}\}/.test(text);

/** Validation message for a value typed as text, or `null` if it fits. Templates are not
 *  judged (their value comes from a variable). */
export function validateText(schema: unknown, text: string, required: boolean): string | null {
  if (text === "") return required ? "required" : null;
  if (isTemplate(text)) return null;
  const s = effective(schema);
  const type = typeOf(s);
  const values = Array.isArray(s.enum) ? s.enum.filter((v) => v !== null).map(String) : [];
  if (values.length > 0 && !values.includes(text)) return `must be one of ${values.join(", ")}`;
  if (type === "integer" || type === "number") {
    if (!/^-?\d+(\.\d+)?([eE][+-]?\d+)?$/.test(text.trim())) return type === "integer" ? "must be a whole number" : "must be a number";
    const n = Number(text);
    if (type === "integer" && !Number.isInteger(n)) return "must be a whole number";
    const lo = lowerBound(s);
    const hi = upperBound(s);
    if (lo && (lo.exclusive ? n <= lo.value : n < lo.value)) return lo.exclusive ? `must be more than ${lo.value}` : `must be at least ${lo.value}`;
    if (hi && (hi.exclusive ? n >= hi.value : n > hi.value)) return hi.exclusive ? `must be less than ${hi.value}` : `must be at most ${hi.value}`;
    const m = num(s.multipleOf);
    if (m !== null && m > 0 && Math.abs(n / m - Math.round(n / m)) > 1e-9) return `must be a multiple of ${m}`;
    return null;
  }
  if (type === "boolean") return text === "true" || text === "false" ? null : "must be true or false";
  if (type === "object" || type === "array") {
    try {
      const errors = validateJson(s, JSON.parse(text));
      return errors.length ? `${errors[0]!.path}: ${errors[0]!.message}` : null;
    } catch {
      return "must be valid JSON";
    }
  }
  const length = [...text].length;
  const min = num(s.minLength);
  const max = num(s.maxLength);
  if (min !== null && length < min) return `must be at least ${min} ${min === 1 ? "character" : "characters"}`;
  if (max !== null && length > max) return `must be at most ${max} characters`;
  if (typeof s.pattern === "string") {
    try {
      if (!new RegExp(s.pattern, "u").test(text)) return `must match ${s.pattern}`;
    } catch {
      // A pattern JavaScript cannot read is not the user's problem.
    }
  }
  return formatProblem(s.format, text);
}

export interface JsonError {
  /** `$.items[0].name`. */
  path: string;
  message: string;
}

function kindOfValue(v: unknown): string {
  if (v === null) return "null";
  if (Array.isArray(v)) return "array";
  if (typeof v === "number") return Number.isInteger(v) ? "integer" : "number";
  return typeof v;
}

function typesOf(s: Schema): string[] {
  const t = s.type;
  const out = typeof t === "string" ? [t] : Array.isArray(t) ? (t as string[]) : [];
  if (s.nullable === true) out.push("null");
  return out;
}

/** Problems of a JSON value against the schema (same rules as the core, at most `limit`). */
export function validateJson(schema: unknown, value: unknown, path = "$", limit = 20, out: JsonError[] = []): JsonError[] {
  if (out.length >= limit) return out;
  const s = effective(schema);
  const types = typesOf(s);
  const actual = kindOfValue(value);
  if (types.length > 0 && !types.includes(actual) && !(actual === "integer" && types.includes("number"))) {
    out.push({ path, message: `expected ${types.join(" or ")}, found ${actual}` });
    return out;
  }
  if (Array.isArray(s.enum) && !s.enum.some((e) => JSON.stringify(e) === JSON.stringify(value))) {
    out.push({ path, message: `must be one of ${s.enum.map((e) => JSON.stringify(e)).join(", ")}` });
  }
  if (typeof value === "string" || typeof value === "number") {
    const scalar: Schema = { ...s };
    delete scalar.enum;
    if (typeof value === "string" && isTemplate(value)) return out;
    const message = validateText(scalar, String(value), false);
    if (message && !message.startsWith("must be one of")) out.push({ path, message });
  } else if (Array.isArray(value)) {
    const min = num(s.minItems);
    const max = num(s.maxItems);
    if (min !== null && value.length < min) out.push({ path, message: `must have at least ${min} ${min === 1 ? "item" : "items"}` });
    if (max !== null && value.length > max) out.push({ path, message: `must have at most ${max} items` });
    if (s.items) value.forEach((v, i) => validateJson(s.items, v, `${path}[${i}]`, limit, out));
  } else if (value && typeof value === "object") {
    const obj = value as Record<string, unknown>;
    const props = asSchema(s.properties);
    for (const name of Array.isArray(s.required) ? (s.required as string[]) : []) {
      if (!(name in obj)) out.push({ path, message: `required property \`${name}\` is missing` });
    }
    for (const [k, v] of Object.entries(obj)) {
      if (k in props) validateJson(props[k], v, `${path}.${k}`, limit, out);
      else if (s.additionalProperties === false) out.push({ path: `${path}.${k}`, message: "property is not in the schema" });
    }
  }
  return out.slice(0, limit);
}

const pad = (n: number) => String(n).padStart(2, "0");

/** A value for "Generate" (or the first enum value, the minimum, an example). */
export function generate(schema: unknown, now: Date = new Date(), uuid: () => string = () => crypto.randomUUID()): string {
  const s = effective(schema);
  const m = describe(s);
  const date = `${now.getUTCFullYear()}-${pad(now.getUTCMonth() + 1)}-${pad(now.getUTCDate())}`;
  const time = `${pad(now.getUTCHours())}:${pad(now.getUTCMinutes())}:${pad(now.getUTCSeconds())}`;
  switch (m.kind) {
    case "uuid":
      return uuid();
    case "date":
      return date;
    case "date-time":
      return `${date}T${time}Z`;
    case "time":
      return time;
    case "email":
      return "user@example.com";
    case "enum":
      return m.options[0] ?? "";
    case "boolean":
      return "true";
    default:
      break;
  }
  if (s.example !== undefined) return typeof s.example === "string" ? s.example : JSON.stringify(s.example);
  if (s.default !== undefined) return typeof s.default === "string" ? s.default : JSON.stringify(s.default);
  if (m.kind === "integer" || m.kind === "number") {
    const lo = lowerBound(s);
    if (lo) return String(lo.exclusive ? lo.value + 1 : lo.value);
    return "1";
  }
  if (m.kind === "json") return JSON.stringify(sample(s), null, 2);
  return "";
}

/** An example value for a schema, as the core's sampler builds it. */
export function sample(schema: unknown, depth = 0): unknown {
  const s = effective(schema);
  if (depth > 6) return null;
  for (const key of ["example", "default", "const"]) if (s[key] !== undefined) return s[key];
  if (Array.isArray(s.enum) && s.enum.length) return s.enum[0];
  const alt = (Array.isArray(s.oneOf) ? s.oneOf : Array.isArray(s.anyOf) ? s.anyOf : [])[0];
  if (alt) return sample(alt, depth + 1);
  switch (typeOf(s)) {
    case "object": {
      const out: Record<string, unknown> = {};
      for (const [k, v] of Object.entries(asSchema(s.properties))) {
        if (asSchema(v).readOnly === true) continue;
        out[k] = sample(v, depth + 1);
      }
      return out;
    }
    case "array":
      return s.items ? [sample(s.items, depth + 1)] : [];
    case "integer":
    case "number":
      return lowerBound(s)?.value ?? 0;
    case "boolean":
      return true;
    case "string": {
      const fixed = new Date(Date.UTC(2024, 0, 1));
      return describe(s).generatable ? generate(s, fixed, () => "00000000-0000-0000-0000-000000000000") : "string";
    }
    default:
      return null;
  }
}

export interface FlatProp {
  name: string;
  schema: Schema;
  required: boolean;
  description: string | null;
}

const SCALARS = new Set(["string", "integer", "number", "boolean"]);

/** The properties of an object schema whose properties are all scalars (or enums), for the
 *  body as a form; `null` if the body is better edited as JSON. At most 20 properties. */
export function flatObject(schema: unknown): FlatProp[] | null {
  const s = effective(schema);
  if (typeOf(s) !== "object") return null;
  const props = Object.entries(asSchema(s.properties));
  if (props.length === 0 || props.length > 20) return null;
  const required = new Set(Array.isArray(s.required) ? (s.required as string[]) : []);
  const out: FlatProp[] = [];
  for (const [name, raw] of props) {
    const p = effective(raw);
    if (p.readOnly === true) continue;
    if (!SCALARS.has(typeOf(p))) return null;
    out.push({ name, schema: p, required: required.has(name), description: typeof p.description === "string" ? p.description : null });
  }
  return out.length ? out : null;
}

/** Form values → the JSON body (typed per property; empty optional fields left out). */
export function bodyFromForm(props: FlatProp[], values: Record<string, string>): string {
  const out: Record<string, unknown> = {};
  for (const p of props) {
    const text = values[p.name] ?? "";
    if (text === "" && !p.required) continue;
    const type = typeOf(p.schema);
    if ((type === "integer" || type === "number") && text !== "" && !Number.isNaN(Number(text)) && !isTemplate(text)) out[p.name] = Number(text);
    else if (type === "boolean" && (text === "true" || text === "false")) out[p.name] = text === "true";
    else out[p.name] = text;
  }
  return JSON.stringify(out, null, 2);
}

/** The JSON body → form values (`null` if it is not an object). */
export function formFromBody(text: string | null, props: FlatProp[]): Record<string, string> | null {
  if (!text || text.trim() === "") return Object.fromEntries(props.map((p) => [p.name, ""]));
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return null;
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return null;
  const obj = parsed as Record<string, unknown>;
  return Object.fromEntries(
    props.map((p) => {
      const v = obj[p.name];
      return [p.name, v === undefined || v === null ? "" : typeof v === "string" ? v : JSON.stringify(v)];
    }),
  );
}
