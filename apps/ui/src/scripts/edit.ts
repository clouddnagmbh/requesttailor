// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Editing scripts, assertions and actions of a request, and "set as variable" from a JSON
// response (wave 7, strand C). Pure functions, so the components stay thin and testable.

import type { Assertion, Script, ScriptPhase, VarAction, VarScope } from "../commands/types";

export const PHASES: { phase: ScriptPhase; label: string; hint: string }[] = [
  { phase: "before-request", label: "Pre-request", hint: "Runs before variables are resolved; may set variables and change the request." },
  { phase: "after-response", label: "Post-response", hint: "Runs after the response; may set variables (they live for the session)." },
  { phase: "tests", label: "Tests", hint: "test(\"name\", () => expect(rt.response.status).to.equal(200));" },
];

/** The code of the first script of `phase` (the editor edits that one). */
export function scriptCode(scripts: Script[], phase: ScriptPhase): string {
  return scripts.find((s) => s.phase === phase)?.code ?? "";
}

/** `scripts` with the first script of `phase` set to `code`; empty code removes it. Other
 *  scripts (more of one phase, as some imports have) stay as they are. */
export function withScriptCode(scripts: Script[], phase: ScriptPhase, code: string): Script[] {
  const at = scripts.findIndex((s) => s.phase === phase);
  if (code.trim() === "") return at < 0 ? scripts : scripts.filter((_, i) => i !== at);
  if (at < 0) return [...scripts, { phase, code }];
  return scripts.map((s, i) => (i === at ? { ...s, code } : s));
}

export function countScripts(scripts: Script[]): number {
  return scripts.filter((s) => s.code.trim() !== "").length;
}

/** Bruno's operators, in its order; unary ones take no value. */
export const OPERATORS = [
  "eq",
  "neq",
  "gt",
  "gte",
  "lt",
  "lte",
  "in",
  "notIn",
  "contains",
  "notContains",
  "length",
  "matches",
  "notMatches",
  "startsWith",
  "endsWith",
  "between",
  "isEmpty",
  "isNotEmpty",
  "isNull",
  "isUndefined",
  "isDefined",
  "isTruthy",
  "isFalsy",
  "isJson",
  "isNumber",
  "isString",
  "isBoolean",
  "isArray",
] as const;

const UNARY = new Set<string>(OPERATORS.slice(OPERATORS.indexOf("isEmpty")));

export function isUnary(operator: string): boolean {
  return UNARY.has(operator);
}

/** A placeholder for the value field that explains the operator's format. */
export function valueHint(operator: string): string {
  switch (operator) {
    case "in":
    case "notIn":
      return "a, b, c";
    case "between":
      return "low, high";
    case "matches":
    case "notMatches":
      return "^regex$";
    case "length":
      return "number";
    default:
      return isUnary(operator) ? "" : "value or {{variable}}";
  }
}

export function newAssertion(): Assertion {
  return { expression: "res.status", operator: "eq", value: "200", description: null, enabled: true };
}

export const SCOPES: { scope: VarScope; label: string }[] = [
  { scope: "runtime", label: "Runtime" },
  { scope: "environment", label: "Environment" },
  { scope: "collection", label: "Collection" },
  { scope: "global", label: "Global" },
];

export const METHODS: { method: string; label: string; hint: string }[] = [
  { method: "jsonq", label: "JSON", hint: "res.body.token" },
  { method: "header", label: "Header", hint: "header name" },
  { method: "status", label: "Status", hint: "" },
];

export function newAction(variable = "", expression = "res.body."): VarAction {
  return { phase: "after-response", expression, method: "jsonq", variable, scope: "runtime", enabled: true };
}

// ---- set as variable (JSON tree) ----

/** The assertion/action expression of a tree path: `$.items[0].id` → `res.body.items[0].id`. */
export function expressionFor(path: string): string {
  return path === "$" ? "res.body" : `res.body${path.slice(1)}`;
}

/** A variable name from the last key of a tree path: `$.user["first name"]` → `firstName`,
 *  `$.items[0]` → `items0`, `$` → `body`. */
export function variableNameFor(path: string): string {
  const keys = [...path.matchAll(/\.([A-Za-z_$][\w$]*)|\["((?:[^"\\]|\\.)*)"\]|\[(\d+)\]/g)].map((m) =>
    m[1] !== undefined ? { key: m[1] } : m[2] !== undefined ? { key: JSON.parse(`"${m[2]}"`) as string } : { index: m[3] },
  );
  let name = "";
  for (let i = keys.length - 1; i >= 0; i--) {
    const k = keys[i];
    if ("key" in k && k.key !== undefined) {
      name = camel(k.key) + name;
      break;
    }
    name = (k.index ?? "") + name;
  }
  if (name === "" || /^\d/.test(name)) name = `body${name}`;
  return name;
}

function camel(key: string): string {
  const parts = key.split(/[^A-Za-z0-9_$]+/).filter((p) => p !== "");
  return parts.map((p, i) => (i === 0 ? p : p[0].toUpperCase() + p.slice(1))).join("");
}

/** A JSON value as a variable value: text as it is, the rest as JSON. */
export function valueText(value: unknown): string {
  return typeof value === "string" ? value : JSON.stringify(value);
}
