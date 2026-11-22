// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// An argument form from a tool's input JSON Schema (wave 8, strand D). Pure functions, so the
// form component stays thin. The form covers the top-level properties of an object schema:
// strings, numbers, integers, booleans and enums get their own control; anything else
// (objects, arrays, unions) is a small JSON field. A schema that is not an object of
// properties has no form: the view offers raw JSON only.
//
// Text may hold `{{variables}}`, resolved by the core when calling. A number field whose
// text is not a number (e.g. a variable) is sent as text.

export type FieldKind = "string" | "number" | "integer" | "boolean" | "enum" | "json";

export interface Field {
  name: string;
  kind: FieldKind;
  required: boolean;
  description: string | null;
  /** `enum` values, as text. */
  options: string[];
  /** The schema's `default`, if any. */
  default?: unknown;
}

type Schema = Record<string, unknown>;

const isObject = (v: unknown): v is Schema => typeof v === "object" && v !== null && !Array.isArray(v);

function kindOf(prop: Schema): FieldKind {
  if (Array.isArray(prop.enum) && prop.enum.every((v) => typeof v === "string")) return "enum";
  let type = prop.type;
  // `["string", "null"]`: the non-null type.
  if (Array.isArray(type)) {
    const real = type.filter((t) => t !== "null");
    type = real.length === 1 ? real[0] : undefined;
  }
  switch (type) {
    case "string":
    case "number":
    case "integer":
    case "boolean":
      return type;
    default:
      return "json";
  }
}

/** The form fields of `schema`, or `null` when it has no form (not an object schema). */
export function fieldsOf(schema: unknown): Field[] | null {
  if (!isObject(schema)) return null;
  if (schema.type !== undefined && schema.type !== "object") return null;
  const props = schema.properties;
  if (props !== undefined && !isObject(props)) return null;
  const required = Array.isArray(schema.required) ? (schema.required as unknown[]) : [];
  return Object.entries(props ?? {}).map(([name, raw]) => {
    const prop = isObject(raw) ? raw : {};
    const field: Field = {
      name,
      kind: kindOf(prop),
      required: required.includes(name),
      description: typeof prop.description === "string" ? prop.description : null,
      options: Array.isArray(prop.enum) ? prop.enum.map(String) : [],
    };
    if ("default" in prop) field.default = prop.default;
    return field;
  });
}

/** Arguments to start with: the defaults, plus `false` for required booleans. */
export function initialArgs(fields: Field[]): Record<string, unknown> {
  const args: Record<string, unknown> = {};
  for (const f of fields) {
    if (f.default !== undefined) args[f.name] = f.default;
    else if (f.kind === "boolean" && f.required) args[f.name] = false;
  }
  return args;
}

/** What a text control shows for `field`. */
export function fieldText(args: Record<string, unknown>, field: Field): string {
  const v = args[field.name];
  if (v === undefined || v === null) return "";
  if (field.kind === "json") return JSON.stringify(v);
  return String(v);
}

/** The value typed into a field: `undefined` removes the argument. `error` when the text
 *  cannot be the field's type (the value is then kept as text). */
export function parseField(field: Field, text: string): { value: unknown; error: string | null } {
  if (text === "") return { value: undefined, error: null };
  switch (field.kind) {
    case "number":
    case "integer": {
      const n = Number(text.trim());
      if (text.trim() !== "" && Number.isFinite(n) && (field.kind === "number" || Number.isInteger(n))) {
        return { value: n, error: null };
      }
      return { value: text, error: text.includes("{{") ? null : `not ${field.kind === "integer" ? "an integer" : "a number"}` };
    }
    case "json":
      try {
        return { value: JSON.parse(text), error: null };
      } catch {
        return { value: text, error: "not valid JSON" };
      }
    default:
      return { value: text, error: null };
  }
}

/** `args` with `name` set to `value` (`undefined`: removed). */
export function withArg(args: Record<string, unknown>, name: string, value: unknown): Record<string, unknown> {
  const next = { ...args };
  if (value === undefined) delete next[name];
  else next[name] = value;
  return next;
}

/** Required fields without a value. */
export function missing(fields: Field[], args: Record<string, unknown>): string[] {
  return fields.filter((f) => f.required && (args[f.name] === undefined || args[f.name] === "")).map((f) => f.name);
}

/** Raw JSON of the arguments: an object, or an error message. */
export function parseRaw(text: string): { args: Record<string, unknown> | null; error: string | null } {
  if (text.trim() === "") return { args: {}, error: null };
  try {
    const v: unknown = JSON.parse(text);
    if (!isObject(v)) return { args: null, error: "the arguments must be a JSON object" };
    return { args: v, error: null };
  } catch (e) {
    return { args: null, error: e instanceof Error ? e.message : String(e) };
  }
}
