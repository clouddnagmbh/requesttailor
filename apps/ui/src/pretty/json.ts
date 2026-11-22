// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Token-level JSON re-indenter (two spaces). It never parses values: numbers and strings are
// copied exactly as written (big numbers keep their digits, `1.0` stays `1.0`, escapes stay
// escapes), so the display and the request body never differ from the bytes. `{{variable}}`
// placeholders are accepted wherever a value or a key may stand and are kept byte-identical.

const INDENT = "  ";
const NUMBER = /^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?$/;
/** Same match as rt-core and Bruno (`\{\{([^}]+)\}\}`), anchored at the position. */
const PLACEHOLDER = /\{\{[^}]+\}\}/y;

interface Frame {
  object: boolean;
  /** first: after the opening bracket; next: after a comma; colon/value: inside a member; after: after a value. */
  state: "first" | "next" | "colon" | "value" | "after";
}

/** A placeholder starting at `i`, or 0. `{{{a}}` is a brace followed by `{{a}}`. */
function placeholderAt(text: string, i: number): number {
  if (text.charCodeAt(i + 1) !== 0x7b || text.charCodeAt(i + 2) === 0x7b) return 0;
  PLACEHOLDER.lastIndex = i;
  return PLACEHOLDER.test(text) ? PLACEHOLDER.lastIndex - i : 0;
}

const isSpace = (c: number) => c === 0x20 || c === 0x0a || c === 0x0d || c === 0x09;
/** Characters that end a bare token (literal, number, placeholder run). */
const isDelimiter = (c: number) =>
  isSpace(c) || c === 0x7b || c === 0x7d || c === 0x5b || c === 0x5d || c === 0x2c || c === 0x3a || c === 0x22;

/**
 * The JSON text re-indented by two spaces, or `null` if it is not valid JSON (placeholders
 * allowed). A trailing newline and a byte order mark are kept.
 */
export function formatJson(text: string): string | null {
  const out: string[] = [];
  const stack: Frame[] = [];
  const indents = [""];
  const newline = (depth: number) => {
    while (indents.length <= depth) indents.push(indents[indents.length - 1] + INDENT);
    return "\n" + indents[depth];
  };
  let done = false;
  let i = 0;
  const n = text.length;
  if (text.charCodeAt(0) === 0xfeff) {
    out.push("﻿");
    i = 1;
  }

  /** Before a value: checks that one may stand here and writes the line break. */
  const value = (): boolean => {
    const f = stack[stack.length - 1];
    if (!f) {
      if (done) return false;
      done = true;
      return true;
    }
    if (f.object) {
      if (f.state !== "value") return false;
    } else {
      if (f.state === "after") return false;
      out.push(newline(stack.length));
    }
    f.state = "after";
    return true;
  };
  /** Before a key: true if the token is a key here (it has been written as one then). */
  const key = (): boolean => {
    const f = stack[stack.length - 1];
    if (!f?.object || (f.state !== "first" && f.state !== "next")) return false;
    out.push(newline(stack.length));
    f.state = "colon";
    return true;
  };

  while (i < n) {
    const c = text.charCodeAt(i);
    if (isSpace(c)) {
      i++;
      continue;
    }
    if (c === 0x22) {
      // String: copied verbatim up to the closing quote.
      let j = i + 1;
      for (;;) {
        if (j >= n) return null;
        const d = text.charCodeAt(j);
        if (d === 0x22) break;
        if (d < 0x20) return null;
        j += d === 0x5c ? 2 : 1;
      }
      const s = text.slice(i, j + 1);
      if (!key() && !value()) return null;
      out.push(s);
      i = j + 1;
      continue;
    }
    if ((c === 0x7b && placeholderAt(text, i) === 0) || c === 0x5b) {
      if (!value()) return null;
      stack.push({ object: c === 0x7b, state: "first" });
      out.push(c === 0x7b ? "{" : "[");
      i++;
      continue;
    }
    if (c === 0x7d || c === 0x5d) {
      const f = stack.pop();
      if (!f || f.object !== (c === 0x7d) || (f.state !== "first" && f.state !== "after")) return null;
      out.push(f.state === "first" ? "" : newline(stack.length), c === 0x7d ? "}" : "]");
      i++;
      continue;
    }
    if (c === 0x2c) {
      const f = stack[stack.length - 1];
      if (f?.state !== "after") return null;
      f.state = f.object ? "next" : "value";
      out.push(",");
      i++;
      continue;
    }
    if (c === 0x3a) {
      const f = stack[stack.length - 1];
      if (f?.state !== "colon") return null;
      f.state = "value";
      out.push(": ");
      i++;
      continue;
    }
    // Bare token: a literal, a number, or a run containing placeholders.
    let j = i;
    let placeholders = false;
    while (j < n) {
      const d = text.charCodeAt(j);
      if (d === 0x7b) {
        const len = placeholderAt(text, j);
        if (len === 0) break;
        placeholders = true;
        j += len;
        continue;
      }
      if (isDelimiter(d)) break;
      j++;
    }
    const token = text.slice(i, j);
    if (placeholders) {
      if (!key() && !value()) return null;
    } else {
      if (!(token === "true" || token === "false" || token === "null" || NUMBER.test(token))) return null;
      if (!value()) return null;
    }
    out.push(token);
    i = j;
  }
  if (!done || stack.length > 0) return null;
  if (text.endsWith("\n")) out.push("\n");
  return out.join("");
}
