// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Counts which script APIs the test corpora use, so the shims of crates/rt-script/js are
// written in order of frequency (wave 7). No dependencies.
//
// Corpora:
// - testdata/collections/bruno: `runtime.scripts` and `request.scripts` of the YAML files
//   (Bruno dialect: `bru.*`, `req.*`, `res.*`, `test`, `expect`). gRPC script types are
//   counted on their own line, they do not run in RequestTailor.
// - testdata/import/postman: `event[].script.exec` (Postman dialect: `pm.*`, `postman.*`,
//   legacy globals such as `tests[...]`, `responseBody`, `responseCode`).
// - testdata/import/insomnia: pre-request and after-response scripts (`insomnia.*`).
// - testdata/import/http: JetBrains `> {% %}` / `< {% %}` handler scripts (`client.*`,
//   `response.*`, `request.*`).
//
// Usage:
//   node scripts/shim-frequency.mjs              table: API, count, files, status
//   node scripts/shim-frequency.mjs --markdown   the same as a Markdown table, with notes
//   node scripts/shim-frequency.mjs --update-doc writes that table into SHIM-COVERAGE.md
//   node scripts/shim-frequency.mjs --dump       every extracted script with its location
//   node scripts/shim-frequency.mjs --unknown    APIs used but missing from the coverage list
//
// Status comes from crates/rt-format/src/import/shim-apis.txt (the list the importers use
// for their loss report).

import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = resolve(fileURLToPath(import.meta.url), "../..");

/** Script types RequestTailor runs (OpenCollection). */
const RUN_TYPES = new Set(["before-request", "after-response", "tests"]);

/** Chai language chains: words without effect. */
export const CHAINS = new Set([
  "to", "be", "been", "is", "that", "which", "and", "has", "have", "with", "at", "of",
  "same", "but", "does", "still", "also",
]);

/** Roots scanned per dialect. Bare identifiers count when used as a value. */
/** Globals any sandbox script may use (timers, web APIs). */
const COMMON = [
  "require", "atob", "btoa", "setTimeout", "setInterval", "clearTimeout", "clearInterval",
  "fetch", "URL", "URLSearchParams", "Buffer", "process", "TextEncoder", "TextDecoder",
];

export const ROOTS = {
  bruno: ["bru", "req", "res", "test", "expect", "__dirname", "__filename", ...COMMON],
  postman: [
    "pm", "postman", "tests", "responseBody", "responseCode", "responseHeaders",
    "responseTime", "responseCookies", "environment", "globals", "iteration", "request",
    "xml2Json", "xmlToJson", "CryptoJS", "_", "cheerio", "tv4", ...COMMON,
  ],
  insomnia: ["insomnia", ...COMMON],
  jetbrains: ["client", "response", "request", "crypto", "$random", "jsonPath", "xpath"],
};

/** Roots that count when used bare (not called, no member). */
const BARE = new Set([
  "responseBody", "responseCode", "responseHeaders", "responseTime", "responseCookies",
  "CryptoJS", "_", "__dirname", "__filename", "iteration",
]);

/** Legacy roots that only count with these members (they are common variable names). */
const MEMBERS_ONLY = {
  request: new Set(["url", "method", "headers", "data", "name", "id", "body", "variables"]),
  environment: null, // any member (Postman's legacy object)
  globals: null,
  iteration: null,
  response: null,
};

// ---------------------------------------------------------------------------------------
// Source cleaning

/** Replaces comments and the contents of string literals with spaces (keeps offsets).
 *  Template literals keep their `${...}` expressions. */
export function blankLiterals(code) {
  const out = code.split("");
  let i = 0;
  const n = code.length;
  const blank = (from, to) => {
    for (let k = from; k < to; k++) if (out[k] !== "\n") out[k] = " ";
  };
  // Whether a `/` at `i` starts a regex literal (previous significant char).
  const regexAllowed = (at) => {
    let k = at - 1;
    while (k >= 0 && /\s/.test(code[k])) k--;
    if (k < 0) return true;
    return /[(,=:[!&|?{};+\-*%<>~^]/.test(code[k]) || /\b(return|typeof|case)$/.test(code.slice(0, k + 1));
  };
  while (i < n) {
    const c = code[i];
    const d = code[i + 1];
    if (c === "/" && d === "/") {
      const end = code.indexOf("\n", i);
      const stop = end < 0 ? n : end;
      blank(i, stop);
      i = stop;
    } else if (c === "/" && d === "*") {
      const end = code.indexOf("*/", i + 2);
      const stop = end < 0 ? n : end + 2;
      blank(i, stop);
      i = stop;
    } else if (c === "'" || c === '"') {
      let k = i + 1;
      while (k < n && code[k] !== c && code[k] !== "\n") k += code[k] === "\\" ? 2 : 1;
      blank(i + 1, Math.min(k, n));
      i = k + 1;
    } else if (c === "`") {
      let k = i + 1;
      let from = k;
      while (k < n && code[k] !== "`") {
        if (code[k] === "\\") k += 2;
        else if (code[k] === "$" && code[k + 1] === "{") {
          blank(from, k);
          let depth = 1;
          k += 2;
          while (k < n && depth > 0) {
            if (code[k] === "{") depth++;
            else if (code[k] === "}") depth--;
            k++;
          }
          from = k;
        } else k++;
      }
      blank(from, Math.min(k, n));
      i = k + 1;
    } else if (c === "/" && regexAllowed(i)) {
      let k = i + 1;
      let cls = false;
      while (k < n && code[k] !== "\n") {
        if (code[k] === "\\") k += 2;
        else if (code[k] === "[") (cls = true), k++;
        else if (code[k] === "]") (cls = false), k++;
        else if (code[k] === "/" && !cls) break;
        else k++;
      }
      if (code[k] === "/") {
        blank(i + 1, k);
        i = k + 1;
      } else i++;
    } else i++;
  }
  return out.join("");
}

// ---------------------------------------------------------------------------------------
// API extraction

const IDENT = /[A-Za-z_$][\w$]*/y;

/** Index after the bracket that closes the one at `open`. */
function closing(text, open) {
  const pairs = { "(": ")", "[": "]", "{": "}" };
  const stack = [pairs[text[open]]];
  let k = open + 1;
  while (k < text.length && stack.length) {
    const c = text[k];
    if (pairs[c]) stack.push(pairs[c]);
    else if (c === stack[stack.length - 1]) stack.pop();
    k++;
  }
  return k;
}

function skipSpace(text, k) {
  while (k < text.length && /\s/.test(text[k])) k++;
  return k;
}

/** Member chain at `k` (after a root or a call): `[segments, end, called]`. */
function chainAt(text, k) {
  const segments = [];
  for (;;) {
    const j = skipSpace(text, k);
    if (text[j] === "." || (text[j] === "?" && text[j + 1] === ".")) {
      const at = skipSpace(text, j + (text[j] === "." ? 1 : 2));
      IDENT.lastIndex = at;
      const m = IDENT.exec(text);
      if (!m) return [segments, k, false];
      segments.push(m[0]);
      k = at + m[0].length;
    } else if (text[j] === "(") {
      return [segments, j, true];
    } else return [segments, k, false];
  }
}

/** Assertion words of an `expect(...)` chain starting at `k`: e.g. `not.equal`. */
function expectChain(text, k) {
  const words = [];
  let end = k;
  for (;;) {
    const [segments, after, called] = chainAt(text, end);
    words.push(...segments.filter((s) => !CHAINS.has(s)));
    if (!called) break;
    end = closing(text, after);
  }
  return words;
}

/** Every API use in `code` for `dialect`: `[{ api, index }]`. `api` is the member chain as
 *  written up to the first call (`pm.environment.get`), `name()` for a called root
 *  (`res()`), `tests[]` for an indexed root, `require('x')` for modules and
 *  `expect().<words>` for chai assertions. */
export function apiUses(code, dialect) {
  const text = blankLiterals(code);
  const roots = ROOTS[dialect];
  const found = [];
  const re = new RegExp(`(?<![\\w$.])(${roots.map((r) => r.replace(/\$/g, "\\$")).join("|")})(?![\\w$])`, "g");
  for (const m of text.matchAll(re)) {
    const root = m[1];
    const start = m.index + root.length;
    // Declarations and parameters (`const res = …`, `(err, res) =>`) are not uses.
    const before = text.slice(0, m.index).trimEnd();
    if (/\b(const|let|var|function)$/.test(before)) continue;
    const after = skipSpace(text, start);
    if (root === "require") {
      if (text[after] !== "(") continue;
      const arg = code.slice(after + 1, closing(text, after) - 1).trim();
      const name = /^['"`]([^'"`]+)['"`]$/.exec(arg);
      found.push({ api: `require('${name ? name[1] : "?"}')`, index: m.index });
      continue;
    }
    if (text[after] === "=" && text[after + 1] !== "=" && text[after + 1] !== ">") continue;
    if (text[after] === "[" && root !== "res" && root !== "req") {
      found.push({ api: `${root}[]`, index: m.index });
      continue;
    }
    const [segments, end, called] = chainAt(text, start);
    const viaRoot = (root === "pm" || root === "insomnia") && segments.length === 1 && segments[0] === "expect";
    if (root === "expect" || (viaRoot && called)) {
      if (!called) continue;
      if (viaRoot) found.push({ api: `${root}.expect`, index: m.index });
      // One use per assertion word: `expect(x).to.not.have.property('a')` → not, property.
      for (const w of expectChain(text, closing(text, end))) found.push({ api: `expect().${w}`, index: m.index });
      continue;
    }
    if (segments.length === 0) {
      if (called) found.push({ api: `${root}()`, index: m.index });
      else if (BARE.has(root)) found.push({ api: root, index: m.index });
      continue;
    }
    // `pm.response.to.have.status` → `pm.response.to.status` (chain words and `not` dropped).
    const to = segments.indexOf("to");
    if ((root === "pm" || root === "insomnia") && segments[0] === "response" && to === 1) {
      const words = segments.slice(2).filter((w) => !CHAINS.has(w) && w !== "not");
      found.push({ api: [root, "response", "to", ...words.slice(0, 1)].join("."), index: m.index });
      continue;
    }
    if (root in MEMBERS_ONLY) {
      const allowed = MEMBERS_ONLY[root];
      if (allowed && !allowed.has(segments[0])) continue;
      if (dialect === "postman" && root === "response") continue;
    }
    found.push({ api: [root, ...segments].join("."), index: m.index });
  }
  return found;
}

// ---------------------------------------------------------------------------------------
// Script extraction

function walk(dir, out = []) {
  for (const name of readdirSync(dir).sort()) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p, out);
    else out.push(p);
  }
  return out;
}

/** Values of `keys` in a YAML text: block scalars (`|`, `|-`, `>`), quoted and plain
 *  scalars. Returns `[{ key, value, line, siblings }]`, `siblings` being the scalar keys of
 *  the same mapping seen before (e.g. `type`). Enough for the corpora; not a YAML parser. */
export function yamlValues(text, keys) {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  const out = [];
  // indent → { key: value } of the mapping currently open at that indent
  let maps = new Map();
  for (let i = 0; i < lines.length; i++) {
    const m = /^(\s*)(- )?([A-Za-z_][\w-]*):(?:\s+(.*))?$/.exec(lines[i]);
    if (!m) continue;
    const indent = m[1].length + (m[2] ? 2 : 0);
    if (m[2]) maps.set(indent, {});
    for (const k of [...maps.keys()]) if (k > indent) maps.delete(k);
    if (!maps.has(indent)) maps.set(indent, {});
    const key = m[3];
    let raw = (m[4] ?? "").trim();
    let value;
    if (/^[|>][-+0-9]*$/.test(raw)) {
      const body = [];
      let j = i + 1;
      let blockIndent = null;
      for (; j < lines.length; j++) {
        const l = lines[j];
        if (l.trim() === "") {
          body.push("");
          continue;
        }
        const ind = l.length - l.trimStart().length;
        if (ind <= indent) break;
        blockIndent ??= ind;
        body.push(l.slice(Math.min(blockIndent, ind)));
      }
      while (body.length && body[body.length - 1] === "") body.pop();
      value = body.join("\n");
      i = j - 1;
    } else if (raw.startsWith('"')) {
      try {
        value = JSON.parse(raw);
      } catch {
        value = raw.slice(1, -1);
      }
    } else if (raw.startsWith("'")) {
      value = raw.slice(1, -1).replace(/''/g, "'");
    } else value = raw;
    const siblings = { ...maps.get(indent) };
    maps.get(indent)[key] = value;
    if (keys.includes(key) && value !== "") out.push({ key, value, line: i + 1, siblings });
  }
  return out;
}

/** Scripts of the Bruno corpus: `[{ file, where, type, code, dialect }]`. */
function brunoScripts() {
  const dir = join(ROOT, "testdata/collections/bruno");
  const out = [];
  for (const file of walk(dir).filter((f) => f.endsWith(".yml"))) {
    const text = readFileSync(file, "utf8");
    if (!/^\s*scripts:/m.test(text)) continue;
    for (const v of yamlValues(text, ["code"])) {
      out.push({ file: relative(ROOT, file), where: `line ${v.line}`, type: v.siblings.type ?? "?", code: v.value, dialect: "bruno" });
    }
  }
  return out;
}

function jsonFiles(dir) {
  return walk(dir).filter((f) => /\.json$/i.test(f));
}

function parseJson(file) {
  try {
    return JSON.parse(readFileSync(file, "utf8").replace(/^﻿/, ""));
  } catch {
    return undefined;
  }
}

/** Scripts of the Postman corpus (`event[].script.exec`). */
function postmanScripts() {
  const out = [];
  for (const file of jsonFiles(join(ROOT, "testdata/import/postman"))) {
    const json = parseJson(file);
    const visit = (v, where) => {
      if (Array.isArray(v)) v.forEach((x, i) => visit(x, `${where}[${i}]`));
      else if (v && typeof v === "object") {
        if (typeof v.listen === "string" && v.script) {
          const exec = v.script.exec;
          const code = Array.isArray(exec) ? exec.join("\n") : (exec ?? "");
          if (code.trim()) out.push({ file: relative(ROOT, file), where, type: v.listen, code, dialect: "postman" });
        }
        for (const [k, x] of Object.entries(v)) visit(x, `${where}.${k}`);
      }
    };
    visit(json, "$");
  }
  return out;
}

/** Scripts of the Insomnia corpus (v4 JSON/YAML, v5 YAML). */
function insomniaScripts() {
  const out = [];
  const keys = ["preRequestScript", "afterResponseScript", "preRequest", "afterResponse"];
  for (const file of walk(join(ROOT, "testdata/import/insomnia"))) {
    if (file.endsWith(".expected")) continue;
    if (file.endsWith(".json")) {
      const json = parseJson(file);
      const visit = (v, where) => {
        if (Array.isArray(v)) v.forEach((x, i) => visit(x, `${where}[${i}]`));
        else if (v && typeof v === "object") {
          for (const [k, x] of Object.entries(v)) {
            if (keys.includes(k) && typeof x === "string" && x.trim()) out.push({ file: relative(ROOT, file), where: `${where}.${k}`, type: k, code: x, dialect: "insomnia" });
            else visit(x, `${where}.${k}`);
          }
        }
      };
      visit(json, "$");
    } else {
      for (const v of yamlValues(readFileSync(file, "utf8"), keys)) {
        out.push({ file: relative(ROOT, file), where: `line ${v.line}`, type: v.key, code: v.value, dialect: "insomnia" });
      }
    }
  }
  return out;
}

/** JetBrains handler scripts of the HTTP corpus. */
function jetbrainsScripts() {
  const out = [];
  for (const file of walk(join(ROOT, "testdata/import/http"))) {
    if (file.endsWith(".expected")) continue;
    const text = readFileSync(file, "utf8");
    for (const m of text.matchAll(/^([<>])\s*\{%([\s\S]*?)%\}/gm)) {
      const line = text.slice(0, m.index).split("\n").length;
      out.push({ file: relative(ROOT, file), where: `line ${line}`, type: m[1] === ">" ? "response-handler" : "pre-request", code: m[2], dialect: "jetbrains" });
    }
  }
  return out;
}

export function corpusScripts() {
  return [...brunoScripts(), ...postmanScripts(), ...insomniaScripts(), ...jetbrainsScripts()];
}

// ---------------------------------------------------------------------------------------
// Coverage list

/** `crates/rt-format/src/import/shim-apis.txt`: `api status note…` per line. */
export function coverageList() {
  const file = join(ROOT, "crates/rt-format/src/import/shim-apis.txt");
  const list = new Map();
  let text;
  try {
    text = readFileSync(file, "utf8");
  } catch {
    return list;
  }
  for (const line of text.split("\n")) {
    const t = line.trim();
    if (!t || t.startsWith("#")) continue;
    const m = /^(\S+)\s+(supported|partial|unsupported)\b\s*(.*)$/.exec(t);
    if (m) list.set(m[1], { status: m[2], note: m[3] });
  }
  return list;
}

/** The coverage entry for `api`: the longest listed entry that is `api` or a member-path
 *  prefix of it (`res.body` covers `res.body.items`). `insomnia.*` falls back to the `pm.*`
 *  entry of the same path (Insomnia's API is Postman's). */
export function coverageOf(api, list) {
  const lookup = (name) => {
    if (list.has(name)) return { api: name, ...list.get(name) };
    const parts = name.split(".");
    for (let k = parts.length - 1; k > 0; k--) {
      const prefix = parts.slice(0, k).join(".");
      if (list.has(prefix)) return { api: prefix, ...list.get(prefix) };
    }
    return undefined;
  };
  const own = lookup(api);
  if (!own && api.startsWith("require(") && list.has("require('*')")) return { api: "require('*')", ...list.get("require('*')") };
  if (own || !api.startsWith("insomnia.")) return own;
  const pm = lookup("pm." + api.slice("insomnia.".length));
  return pm && { ...pm, api: "insomnia." + pm.api.slice(3) };
}

// ---------------------------------------------------------------------------------------

/** The readable coverage table; the frequency table sits between these markers. */
export const COVERAGE_DOC = join(ROOT, "crates/rt-script/SHIM-COVERAGE.md");
export const DOC_START = "<!-- frequency:start (node scripts/shim-frequency.mjs --update-doc) -->";
export const DOC_END = "<!-- frequency:end -->";

/** The frequency table as `--markdown` prints it. */
export function frequencyMarkdown() {
  const lines = [];
  const log = console.log;
  console.log = (s) => lines.push(s);
  try {
    main(["--markdown"]);
  } finally {
    console.log = log;
  }
  return lines.join("\n");
}

function main(args) {
  const scripts = corpusScripts();
  if (args.includes("--dump")) {
    for (const s of scripts) console.log(`=== ${s.file} ${s.where} (${s.type})\n${s.code}\n`);
    return;
  }
  const list = coverageList();
  const counts = new Map();
  let grpc = 0;
  const scanned = { bruno: 0, postman: 0, insomnia: 0, jetbrains: 0 };
  for (const s of scripts) {
    if (s.dialect === "bruno" && !RUN_TYPES.has(s.type)) {
      grpc++;
      continue;
    }
    scanned[s.dialect]++;
    for (const { api } of apiUses(s.code, s.dialect)) {
      const c = coverageOf(api, list);
      // Unknown modules keep their name; everything else counts under its list entry.
      const name = c && c.api !== "require('*')" ? c.api : api;
      const key = `${s.dialect}\t${name}`;
      const e = counts.get(key) ?? { dialect: s.dialect, api: name, count: 0, files: new Set(), status: c?.status ?? "unlisted", note: c?.note ?? "" };
      e.count++;
      e.files.add(s.file);
      counts.set(key, e);
    }
  }
  const rows = [...counts.values()].sort((a, b) => b.count - a.count || a.dialect.localeCompare(b.dialect) || a.api.localeCompare(b.api));
  if (args.includes("--unknown")) {
    for (const r of rows.filter((r) => r.status === "unlisted")) console.log(`${r.dialect}\t${r.api}\t${r.count}`);
    return;
  }
  const summary =
    `Scripts scanned: Bruno ${scanned.bruno} (plus ${grpc} gRPC scripts, not run), ` +
    `Postman ${scanned.postman}, Insomnia ${scanned.insomnia}, JetBrains HTTP ${scanned.jetbrains}.`;
  const markdown = [
    summary,
    "",
    "| Dialect | API | Uses | Files | Status | Note |",
    "| --- | --- | ---: | ---: | --- | --- |",
    ...rows.map((r) => `| ${r.dialect} | \`${r.api}\` | ${r.count} | ${r.files.size} | ${r.status} | ${r.note.replaceAll("|", "\\|")} |`),
  ].join("\n");
  if (args.includes("--markdown")) {
    console.log(markdown);
    return;
  }
  if (args.includes("--update-doc")) {
    const doc = readFileSync(COVERAGE_DOC, "utf8");
    const [start, end] = [doc.indexOf(DOC_START), doc.indexOf(DOC_END)];
    if (start < 0 || end < start) throw new Error(`${COVERAGE_DOC}: markers ${DOC_START} and ${DOC_END} not found`);
    writeFileSync(COVERAGE_DOC, `${doc.slice(0, start + DOC_START.length)}\n${markdown}\n${doc.slice(end)}`);
    return;
  }
  console.log(summary);
  for (const r of rows) console.log(`${String(r.count).padStart(5)}  ${String(r.files.size).padStart(4)}  ${r.dialect.padEnd(9)}  ${r.api.padEnd(48)}  ${r.status}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2));
}
