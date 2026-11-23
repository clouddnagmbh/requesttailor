// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Checks the static website in website/ (no dependencies):
// - nothing is loaded from other servers and nothing runs: no scripts, frames, forms or
//   embeds; stylesheets, images and CSS url()/@import are relative files that exist; every
//   page carries a Content-Security-Policy meta tag starting with default-src 'none'
//   (product principle 2: no trackers, no external fonts or scripts);
// - every page has the basics for accessibility: lang, viewport, title, one h1, a main
//   landmark and a skip link to it;
// - internal links point to files that exist, and their #fragments to ids that exist;
// - the performance budgets shown (data-budget="<metric>") are the ones in perf/budgets.toml,
//   and the home page shows all of them.
// Usage: node scripts/check-website.mjs [website-dir] [budgets.toml]

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { parseBudgets } from "./check-perf-budgets.mjs";

const HEADER = "SPDX-License-Identifier: Apache-2.0";
const FORBIDDEN_TAGS = ["script", "iframe", "frame", "object", "embed", "form", "base"];
// A URL with a scheme (https:, data:, ...) or protocol-relative (//host).
const ABSOLUTE = /^([a-z][a-z0-9+.-]*:|\/\/)/i;

function walk(dir, out = []) {
  for (const name of readdirSync(dir).sort()) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path, out);
    else out.push(path);
  }
  return out;
}

/** `max` per metric from perf/budgets.toml, parsed as the budget gate parses it. A budget still
 *  marked `pending` (its measurement is not built yet, since wave 15) is no promise yet: the
 *  site shows it once the strand removes `pending`. */
export function budgetMaxima(text) {
  return Object.fromEntries(
    Object.entries(parseBudgets(text))
      .filter(([, m]) => typeof m.pending !== "string")
      .map(([name, m]) => [name, m.max]),
  );
}

function attr(tag, name) {
  const m = tag.match(new RegExp(`\\s${name}\\s*=\\s*("([^"]*)"|'([^']*)')`, "i"));
  return m ? (m[2] ?? m[3]) : null;
}

function ids(html) {
  return new Set([...html.matchAll(/\sid\s*=\s*"([^"]+)"/g)].map((m) => m[1]));
}

/** The file a relative URL from `page` points to (a directory means its index.html). */
function target(root, page, url) {
  const path = decodeURIComponent(url.split("#")[0].split("?")[0]);
  if (path === "") return page;
  let file = resolve(dirname(page), path);
  if (path.endsWith("/") || (existsSync(file) && statSync(file).isDirectory())) {
    file = join(file, "index.html");
  }
  return file;
}

/**
 * Checks the site below `root` against `budgets` (metric -> max). Returns a list of
 * problems, empty when everything is fine.
 */
export function checkWebsite(root, budgets) {
  const errors = [];
  const files = walk(root);
  const pages = files.filter((f) => f.endsWith(".html"));
  const styles = files.filter((f) => f.endsWith(".css"));
  const name = (f) => relative(root, f).split(sep).join("/");
  const inside = (f) => resolve(f).startsWith(resolve(root) + sep);
  if (pages.length === 0) errors.push("no pages found");
  const idCache = new Map();
  const idsOf = (file) => {
    if (!idCache.has(file)) idCache.set(file, ids(readFileSync(file, "utf8")));
    return idCache.get(file);
  };

  for (const page of pages) {
    const p = name(page);
    const html = readFileSync(page, "utf8");
    const head = html.split("\n", 5).join("\n");
    if (!/^<!doctype html>/i.test(html)) errors.push(`${p}: does not start with <!doctype html>`);
    if (!head.includes(HEADER)) errors.push(`${p}: no "${HEADER}" in the first lines`);
    if (!/<html\s[^>]*lang="[a-z-]+"/i.test(html)) errors.push(`${p}: <html> has no lang`);
    if (!/<meta charset="utf-8">/i.test(html)) errors.push(`${p}: no <meta charset="utf-8">`);
    if (!/<meta name="viewport"/i.test(html)) errors.push(`${p}: no viewport meta tag`);
    const csp = html.match(/<meta http-equiv="Content-Security-Policy" content="([^"]*)"/i);
    if (!csp || !csp[1].startsWith("default-src 'none'")) {
      errors.push(`${p}: no Content-Security-Policy meta tag with default-src 'none'`);
    } else if (/https?:|\*/.test(csp[1])) {
      errors.push(`${p}: the Content-Security-Policy allows other servers`);
    }
    if (!/<title>[^<]+<\/title>/i.test(html)) errors.push(`${p}: no <title>`);
    const h1 = (html.match(/<h1[\s>]/gi) ?? []).length;
    if (h1 !== 1) errors.push(`${p}: ${h1} <h1> elements, expected 1`);
    if (!/<main\s[^>]*id="main"/i.test(html)) errors.push(`${p}: no <main id="main">`);
    if (!html.includes('href="#main"')) errors.push(`${p}: no skip link to #main`);
    for (const tag of FORBIDDEN_TAGS) {
      if (new RegExp(`<${tag}[\\s>]`, "i").test(html)) errors.push(`${p}: <${tag}> is not allowed`);
    }
    if (/\son[a-z]+\s*=/i.test(html)) errors.push(`${p}: inline event handlers are not allowed`);
    if (/\sstyle\s*=/i.test(html) || /<style[\s>]/i.test(html)) {
      errors.push(`${p}: inline styles are not allowed (the CSP blocks them); use assets/site.css`);
    }

    // Resources: must be local files.
    for (const m of html.matchAll(/<(link|img|source|video|audio|track|input)\b[^>]*>/gi)) {
      const tag = m[0];
      for (const key of ["href", "src", "srcset", "poster"]) {
        const value = attr(tag, key);
        if (value === null) continue;
        const urls = key === "srcset" ? value.split(",").map((s) => s.trim().split(/\s+/)[0]) : [value];
        for (const url of urls) {
          if (ABSOLUTE.test(url) || url.startsWith("/")) {
            errors.push(`${p}: ${m[1]} ${key}="${url}" is not a relative local file`);
          } else if (!existsSync(target(root, page, url)) || !inside(target(root, page, url))) {
            errors.push(`${p}: ${m[1]} ${key}="${url}" does not exist`);
          }
        }
      }
    }

    // Links: internal ones must resolve, external ones must be https or mailto.
    for (const m of html.matchAll(/<a\b[^>]*>/gi)) {
      const href = attr(m[0], "href");
      if (href === null) {
        errors.push(`${p}: <a> without href`);
        continue;
      }
      if (ABSOLUTE.test(href)) {
        if (!/^(https:\/\/|mailto:)/i.test(href)) errors.push(`${p}: link ${href} is not https`);
        continue;
      }
      if (href.startsWith("/")) {
        errors.push(`${p}: link ${href} is root-relative; use a relative link (the site may live in a subfolder)`);
        continue;
      }
      const file = target(root, page, href);
      if (!inside(file) || !existsSync(file)) {
        errors.push(`${p}: link ${href} points to a missing file`);
        continue;
      }
      const fragment = href.includes("#") ? href.slice(href.indexOf("#") + 1) : "";
      if (fragment && file.endsWith(".html") && !idsOf(file).has(fragment)) {
        errors.push(`${p}: link ${href}: no id "${fragment}" in ${name(file)}`);
      }
    }

    // Budgets.
    for (const m of html.matchAll(/data-budget="([a-z0-9_]+)"[^>]*>([^<]*)</g)) {
      const [, metric, shown] = m;
      if (!(metric in budgets)) errors.push(`${p}: unknown budget ${metric}`);
      else if (Number(shown.trim()) !== budgets[metric]) {
        errors.push(`${p}: budget ${metric} shows ${shown.trim()}, perf/budgets.toml says ${budgets[metric]}`);
      }
    }
  }

  const home = join(root, "index.html");
  if (existsSync(home)) {
    const html = readFileSync(home, "utf8");
    for (const metric of Object.keys(budgets)) {
      if (!html.includes(`data-budget="${metric}"`)) {
        errors.push(`index.html: budget ${metric} from perf/budgets.toml is not shown`);
      }
    }
  } else {
    errors.push("no index.html");
  }

  for (const style of styles) {
    const s = name(style);
    const css = readFileSync(style, "utf8");
    if (!css.split("\n", 5).join("\n").includes(HEADER)) errors.push(`${s}: no "${HEADER}" in the first lines`);
    for (const m of css.matchAll(/@import\s+(?:url\()?\s*["']?([^"')\s;]+)/gi)) {
      errors.push(`${s}: @import ${m[1]} is not allowed; keep one stylesheet`);
    }
    for (const m of css.matchAll(/url\(\s*["']?([^"')]+)["']?\s*\)/gi)) {
      const url = m[1];
      if (ABSOLUTE.test(url) || url.startsWith("/")) errors.push(`${s}: url(${url}) is not a relative local file`);
      else if (!existsSync(resolve(dirname(style), url))) errors.push(`${s}: url(${url}) does not exist`);
    }
  }
  return errors;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
  const root = resolve(process.argv[2] ?? join(repo, "website"));
  const budgetsFile = resolve(process.argv[3] ?? join(repo, "perf", "budgets.toml"));
  const errors = checkWebsite(root, budgetMaxima(readFileSync(budgetsFile, "utf8")));
  if (errors.length > 0) {
    console.error(`website check failed (${errors.length}):`);
    for (const e of errors) console.error(`  ${e}`);
    process.exit(1);
  }
  const pages = walk(root).filter((f) => f.endsWith(".html")).length;
  console.log(`website ok: ${pages} pages`);
}
