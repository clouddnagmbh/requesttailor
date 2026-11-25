// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { test } from "node:test";
import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { budgetMaxima, canonicalOf, checkWebsite } from "./check-website.mjs";

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
    change(root, "docs/mcp/index.html", "<h1>MCP</h1>", "<h2>MCP</h2>");
  });
  assert.ok(errors.includes('docs/privacy/index.html: no <main id="main">'), errors.join("\n"));
  assert.ok(errors.includes("docs/mcp/index.html: 0 <h1> elements, expected 1"), errors.join("\n"));
});

test("a JSON-LD data block is allowed, every other script is not", () => {
  const ld = '<script type="application/ld+json">{"@context":"https://schema.org","@type":"Thing"}</script>\n</head>';
  assert.deepEqual(withCopy((root) => change(root, "docs/cli/index.html", "</head>", ld)), []);

  const errors = withCopy((root) => {
    change(root, "index.html", "</head>", "<script>alert(1)</script>\n</head>");
    change(root, "sap/index.html", "</head>", '<script type="module">import "./a.js"</script>\n</head>');
    change(root, "docs/index.html", "</head>", '<script type="application/ld+json" src="x.json"></script>\n</head>');
    change(root, "download/index.html", "</head>", '<script type="application/ld+json">{not json</script>\n</head>');
    change(root, "run/index.html", "</head>", '<script type="application/ld+json" src="https://example.com/a.json">{}</script>\n</head>');
  });
  for (const page of ["index.html", "sap/index.html", "docs/index.html", "run/index.html"]) {
    assert.ok(errors.some((e) => e.startsWith(`${page}: <script> is not allowed`)), errors.join("\n"));
  }
  assert.ok(errors.some((e) => e.startsWith("download/index.html: JSON-LD does not parse")), errors.join("\n"));
});

test("canonical and social URLs may name only the two project origins", () => {
  const errors = withCopy((root) => {
    change(root, "index.html", 'href="https://requesttailor.com/"', 'href="https://example.com/"');
    change(root, "sap/index.html", 'content="https://requesttailor.com/assets/og-image.png"', 'content="https://cdn.example.com/og.png"');
    change(root, "docs/cli/index.html", '<link rel="canonical" href="https://requesttailor.com/docs/cli/">', '<link rel="canonical" href="https://requesttailor.com/docs/cli/">\n<link rel="stylesheet" href="https://requesttailor.com/assets/site.css">');
  });
  assert.ok(errors.some((e) => e.startsWith("index.html: canonical https://example.com/")), errors.join("\n"));
  assert.ok(errors.some((e) => e.includes("sap/index.html: og:image https://cdn.example.com/og.png is not on")), errors.join("\n"));
  // Everything else with an absolute URL in a <link> stays forbidden.
  assert.ok(errors.some((e) => e.includes('docs/cli/index.html: link href="https://requesttailor.com/assets/site.css"')), errors.join("\n"));
  // The two project origins are fine.
  assert.deepEqual(
    withCopy((root) => change(root, "index.html", 'content="https://requesttailor.com/assets/og-image.png"', 'content="https://requesttailor.com/assets/og-image.png"')),
    [],
  );
});

test("search metadata is required and limited", () => {
  const long = "x".repeat(70);
  const errors = withCopy((root) => {
    change(root, "download/index.html", "<title>Download RequestTailor for macOS and the rt CLI</title>", `<title>${long}</title>`);
    change(root, "run/index.html", '<link rel="canonical" href="https://requesttailor.com/run/">', '<link rel="canonical" href="https://requesttailor.com/runs/">');
    change(root, "benchmark/index.html", '<meta property="og:image:alt"', '<meta property="og:imagealt"');
    change(root, "docs/mcp/index.html", '<img src="../../assets/logo-mark.svg" alt="" width="24" height="24">', '<img src="../../assets/logo-mark.svg" width="24" height="24">');
  });
  assert.ok(errors.some((e) => e.startsWith("download/index.html: title is 70 characters")), errors.join("\n"));
  assert.ok(errors.some((e) => e.startsWith("run/index.html: canonical link is https://requesttailor.com/runs/")), errors.join("\n"));
  assert.ok(errors.some((e) => e === "benchmark/index.html: no og:image:alt meta tag"), errors.join("\n"));
  assert.ok(errors.some((e) => e.startsWith("docs/mcp/index.html: <img> without alt")), errors.join("\n"));
  assert.equal(canonicalOf("docs/cli/index.html"), "https://requesttailor.com/docs/cli/");
  assert.equal(canonicalOf("index.html"), "https://requesttailor.com/");
});

test("duplicate titles, duplicate ids and a stale sitemap are found", () => {
  const errors = withCopy((root) => {
    const home = readFileSync(join(root, "index.html"), "utf8");
    const title = home.match(/<title>([^<]*)<\/title>/)[1];
    change(root, "run/index.html", "<title>Run in RequestTailor: open a repository's API requests</title>", `<title>${title}</title>`);
    change(root, "docs/privacy/index.html", '<h2 id="data">', '<h2 id="app">');
    writeFileSync(join(root, "sitemap.xml"), readFileSync(join(root, "sitemap.xml"), "utf8").replace("<loc>https://requesttailor.com/sap/</loc>", "<loc>https://requesttailor.com/gone/</loc>"));
    writeFileSync(join(root, "robots.txt"), "User-agent: *\nAllow: /\n");
  });
  assert.ok(errors.some((e) => e.startsWith("run/index.html: title") && e.includes("is also used by")), errors.join("\n"));
  assert.ok(errors.some((e) => e === 'docs/privacy/index.html: duplicate id "app"'), errors.join("\n"));
  assert.ok(errors.some((e) => e.startsWith("sitemap.xml:") && e.includes("/sap/") && e.includes("/gone/")), errors.join("\n"));
  assert.ok(errors.includes("robots.txt: no Sitemap line"), errors.join("\n"));
});
