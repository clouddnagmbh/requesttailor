// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// npm license gate. Reads every package recorded in package-lock.json (including dev
// dependencies) and fails if a license is missing or not on the allowlist.
// The allowlist is the [licenses].allow array in deny.toml, so Rust and npm share one policy.
//
// Usage: node scripts/check-npm-licenses.mjs [path/to/package-lock.json] [path/to/deny.toml]
// No dependencies on purpose: the gate must not need the packages it is checking.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

/** Extracts the [licenses].allow array from deny.toml. */
export function readAllowlist(denyToml) {
  const section = denyToml.match(/^\[licenses\]\s*$([\s\S]*?)(?=^\[|(?![\s\S]))/m);
  if (!section) throw new Error("deny.toml: no [licenses] section");
  const allow = section[1].match(/^allow\s*=\s*\[([\s\S]*?)\]/m);
  if (!allow) throw new Error("deny.toml: no [licenses].allow array");
  const body = allow[1].replace(/#.*$/gm, "");
  const ids = [...body.matchAll(/"([^"]+)"/g)].map((m) => m[1]);
  if (ids.length === 0) throw new Error("deny.toml: [licenses].allow is empty");
  return new Set(ids);
}

function tokenize(expr) {
  return expr.replace(/\(/g, " ( ").replace(/\)/g, " ) ").trim().split(/\s+/).filter(Boolean);
}

/**
 * Evaluates an SPDX license expression against the allowlist.
 * OR is satisfied if one side is allowed, AND only if both are. "X WITH Y" must be listed as a whole.
 * Throws on malformed expressions.
 */
export function isAllowed(expr, allowed) {
  const tokens = tokenize(expr);
  let pos = 0;
  const peek = () => tokens[pos];
  const next = () => tokens[pos++];

  function primary() {
    const t = next();
    if (t === undefined) throw new Error(`unexpected end of expression: "${expr}"`);
    if (t === "(") {
      const v = orExpr();
      if (next() !== ")") throw new Error(`missing ")" in "${expr}"`);
      return v;
    }
    if (t === ")" || t === "AND" || t === "OR" || t === "WITH") {
      throw new Error(`unexpected "${t}" in "${expr}"`);
    }
    if (peek() === "WITH") {
      next();
      const exception = next();
      if (exception === undefined) throw new Error(`missing exception after WITH in "${expr}"`);
      return allowed.has(`${t} WITH ${exception}`);
    }
    return allowed.has(t);
  }
  function andExpr() {
    let v = primary();
    while (peek() === "AND") {
      next();
      v = primary() && v;
    }
    return v;
  }
  function orExpr() {
    let v = andExpr();
    while (peek() === "OR") {
      next();
      v = andExpr() || v;
    }
    return v;
  }

  const result = orExpr();
  if (pos !== tokens.length) throw new Error(`trailing tokens in "${expr}"`);
  return result;
}

function licenseOf(entry) {
  const l = entry.license;
  if (typeof l === "string" && l.trim() !== "") return l.trim();
  if (l && typeof l === "object" && typeof l.type === "string") return l.type.trim();
  return null;
}

/** Returns a list of violations ({ name, license, reason }) for a parsed package-lock.json. */
export function checkLockfile(lock, allowed) {
  if (!lock || typeof lock.packages !== "object") {
    throw new Error("package-lock.json has no \"packages\" map (lockfileVersion >= 2 required)");
  }
  const violations = [];
  for (const [path, entry] of Object.entries(lock.packages)) {
    if (entry.link) continue; // symlink to a workspace package; the target is checked itself
    const name = entry.name ?? (path.split("node_modules/").pop() || "(root)");
    const license = licenseOf(entry);
    if (license === null) {
      violations.push({ name, license: "(none)", reason: "no license declared" });
      continue;
    }
    let ok;
    try {
      ok = isAllowed(license, allowed);
    } catch (err) {
      violations.push({ name, license, reason: `unparseable: ${err.message}` });
      continue;
    }
    if (!ok) violations.push({ name, license, reason: "not on the allowlist in deny.toml" });
  }
  return violations;
}

function main() {
  const lockPath = resolve(process.argv[2] ?? "package-lock.json");
  const denyPath = resolve(process.argv[3] ?? "deny.toml");
  const allowed = readAllowlist(readFileSync(denyPath, "utf8"));
  const lock = JSON.parse(readFileSync(lockPath, "utf8"));
  const total = Object.values(lock.packages ?? {}).filter((e) => !e.link).length;
  const violations = checkLockfile(lock, allowed);

  if (violations.length > 0) {
    console.error(`npm license check FAILED for ${lockPath}:`);
    for (const v of violations) console.error(`  ${v.name}: ${v.license} (${v.reason})`);
    process.exit(1);
  }
  console.log(`npm license check ok: ${total} packages in ${lockPath}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main();
}
