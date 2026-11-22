// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { test } from "node:test";
import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { budgetMaxima, checkWebsite } from "./check-website.mjs";

const SITE = fileURLToPath(new URL("../website", import.meta.url));
const budgets = budgetMaxima(readFileSync(new URL("../perf/budgets.toml", import.meta.url), "utf8"));

test("the website passes", () => {
  assert.deepEqual(checkWebsite(SITE, budgets), []);
});

test("the website has the pages the launch needs", () => {
  for (const page of ["index.html", "sap/index.html", "download/index.html", "docs/index.html"]) {
    assert.ok(readFileSync(join(SITE, page), "utf8").includes("<h1"), page);
  }
});

/** A copy of the site in a temporary folder, changed by `edit(root)`; returns the problems. */
function withCopy(edit) {
  const root = mkdtempSync(join(tmpdir(), "rt-website-"));
  try {
    cpSync(SITE, root, { recursive: true });
    edit(root);
    return checkWebsite(root, budgets);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function change(root, page, from, to) {
  const file = join(root, page);
  const text = readFileSync(file, "utf8");
  assert.ok(text.includes(from), `${page} contains ${from}`);
  writeFileSync(file, text.replace(from, to));
}

test("external scripts, fonts and trackers are refused", () => {
  const errors = withCopy((root) => {
    change(root, "index.html", "</head>", '<script src="https://cdn.example.com/a.js"></script>\n</head>');
    change(
      root,
      "sap/index.html",
      '<link rel="stylesheet" href="../assets/site.css">',
      '<link rel="stylesheet" href="https://fonts.example.com/css">',
    );
    change(root, "docs/index.html", "</article>", '<img src="https://track.example.com/p.gif" alt=""></article>');
  });
  assert.ok(errors.some((e) => e.startsWith("index.html: <script>")), errors.join("\n"));
  assert.ok(errors.some((e) => e.includes("sap/index.html: link href=\"https://fonts")), errors.join("\n"));
  assert.ok(errors.some((e) => e.includes("docs/index.html: img src=\"https://track")), errors.join("\n"));
});

test("external url() in CSS and a relaxed CSP are refused", () => {
  const errors = withCopy((root) => {
    change(root, "assets/site.css", "body {", "body {\n  background-image: url(https://example.com/bg.png);");
    change(root, "download/index.html", "default-src 'none';", "default-src 'none'; img-src https:;");
  });
  assert.ok(errors.some((e) => e.includes("site.css: url(https://example.com/bg.png)")), errors.join("\n"));
  assert.ok(errors.some((e) => e.includes("download/index.html: the Content-Security-Policy")), errors.join("\n"));
});

test("broken internal links and fragments are found", () => {
  const errors = withCopy((root) => {
    change(root, "index.html", 'href="download/"', 'href="downloads/"');
    change(root, "docs/cli/index.html", 'href="../environments/#scanner"', 'href="../environments/#scanners"');
  });
  assert.ok(errors.some((e) => e.includes("link downloads/ points to a missing file")), errors.join("\n"));
  assert.ok(errors.some((e) => e.includes('no id "scanners"')), errors.join("\n"));
});

test("budgets must match perf/budgets.toml", () => {
  const errors = withCopy((root) => {
    change(root, "index.html", '<span data-budget="cold_start">500</span>', '<span data-budget="cold_start">400</span>');
  });
  assert.deepEqual(errors, [
    "index.html: budget cold_start shows 400, perf/budgets.toml says 500",
  ]);
  const missing = checkWebsite(SITE, { ...budgets, new_metric: 1 });
  assert.deepEqual(missing, ["index.html: budget new_metric from perf/budgets.toml is not shown"]);
});

test("pages need a title, one h1 and a main landmark", () => {
  const errors = withCopy((root) => {
    change(root, "docs/privacy/index.html", '<main id="main">', "<main>");
    change(root, "docs/mcp/index.html", "<h1>MCP server</h1>", "<h2>MCP server</h2>");
  });
  assert.ok(errors.includes('docs/privacy/index.html: no <main id="main">'), errors.join("\n"));
  assert.ok(errors.includes("docs/mcp/index.html: 0 <h1> elements, expected 1"), errors.join("\n"));
});
