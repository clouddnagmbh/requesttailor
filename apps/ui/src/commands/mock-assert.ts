// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Assertions and set-variable actions in the mock backend (wave 7, strand C): a tiny model of
// `rt_core::assert`, good enough for UI tests. Expressions: `res.status`, `res.body` with a
// dotted path (`res.body.a.b[0]`), or a variable. Operators `eq` and `neq` (numbers compare
// numerically, `"…"` is text); anything else fails with a note that the mock does not
// evaluate it. The real rules live in Rust and are tested there.

import type { Assertion, AssertionResult, VarAction, VarChange } from "./types";

interface MockResponse {
  status: number;
  body: string;
}

type Lookup = (name: string) => string | undefined;

const UNDEFINED = Symbol("undefined");
type Val = unknown | typeof UNDEFINED;

function parseBody(body: string): unknown {
  try {
    return JSON.parse(body);
  } catch {
    return body;
  }
}

function walk(value: unknown, path: string): Val {
  const keys = path.match(/[^.[\]]+/g) ?? [];
  let cur: unknown = value;
  for (const key of keys) {
    if (cur !== null && typeof cur === "object" && key in (cur as object)) {
      cur = (cur as Record<string, unknown>)[key];
    } else if ((Array.isArray(cur) || typeof cur === "string") && key === "length") {
      cur = cur.length;
    } else {
      return UNDEFINED;
    }
  }
  return cur;
}

function evaluateExpression(expression: string, response: MockResponse, lookup: Lookup): Val {
  const e = expression.trim();
  if (e === "res.status") return response.status;
  if (e === "res.body") return parseBody(response.body);
  if (e.startsWith("res.body.") || e.startsWith("res.body[")) return walk(parseBody(response.body), e.slice(8));
  const v = lookup(e);
  return v === undefined ? UNDEFINED : literal(v);
}

function literal(raw: string): unknown {
  const s = raw.trim();
  if (s === "true") return true;
  if (s === "false") return false;
  if (s === "null") return null;
  if (s.length >= 2 && /^(".*"|'.*')$/.test(s)) return s.slice(1, -1);
  if (s !== "" && !Number.isNaN(Number(s))) return Number(s);
  return s;
}

const text = (v: Val): string | null =>
  v === UNDEFINED ? null : typeof v === "string" ? v : JSON.stringify(v);

const resolve = (template: string, lookup: Lookup) =>
  template.replace(/\{\{([^}]+)\}\}/g, (whole, name: string) => lookup(name) ?? whole);

export function mockEvaluate(assertions: Assertion[], response: MockResponse, lookup: Lookup): AssertionResult[] {
  return assertions
    .filter((a) => a.enabled)
    .map((a) => {
      const operator = a.operator.trim() || "eq";
      const expected = resolve(a.value, lookup);
      const actual = evaluateExpression(a.expression, response, lookup);
      const result: AssertionResult = {
        expression: a.expression,
        operator: a.operator,
        expected,
        actual: text(actual),
        passed: false,
      };
      if (operator !== "eq" && operator !== "neq") {
        result.error = `the mock backend evaluates only eq and neq, not \`${operator}\``;
        return result;
      }
      const same = actual !== UNDEFINED && actual === literal(expected);
      result.passed = operator === "eq" ? same : !same;
      if (!result.passed && actual === UNDEFINED) {
        result.error = `\`${a.expression.trim()}\` did not resolve (no such value)`;
      }
      return result;
    });
}

export function mockActions(actions: VarAction[], response: MockResponse): VarChange[] {
  return actions
    .filter((a) => a.enabled && a.phase !== "before-request" && a.variable.trim() !== "")
    .flatMap((a): VarChange[] => {
      const value =
        a.method === "status"
          ? response.status
          : a.method === "jsonq"
            ? evaluateExpression(a.expression, response, () => undefined)
            : UNDEFINED;
      const t = text(value);
      return t === null ? [] : [{ name: a.variable.trim(), value: t, scope: a.scope }];
    });
}
