<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# Website

The project website: plain HTML and one stylesheet, no build step, no framework.

| Path | Page |
| --- | --- |
| `index.html` | What RequestTailor is: principles, performance budgets, surfaces |
| `sap/` | The OData plugin for S/4HANA, CAP and RAP |
| `download/` | Release files per OS, checksum and signature verification |
| `compare/` | What RequestTailor does and does not do, as a yes/no list |
| `docs/` | Getting started, collections, environments and secrets, Git client, checks, safety and quality tools, sharing, AI assistants, CLI, MCP, plugins, OData guide, privacy |
| `404.html` | Page not found (not indexed; its links, stylesheet and logo are absolute because it is served at any path) |
| `sitemap.xml`, `robots.txt` | For crawlers; the sitemap lists every page (the check compares) and robots.txt names it |
| `llms.txt`, `llms-full.txt` | For answer engines ([llmstxt.org](https://llmstxt.org/)): key pages, and all facts in one file |
| `assets/og-image.png` | The 1200 x 630 image for link previews, made from the logo |
| `assets/site.css` | The only stylesheet; light and dark follow `prefers-color-scheme` |

## Rules

- Nothing is loaded from other servers and nothing runs: no scripts, no web fonts, no
  analytics, no cookies, no embeds (product principle 2). Every page has a
  `Content-Security-Policy` meta tag with `default-src 'none'` that enforces this in the
  browser.
- The one `<script>` allowed is a JSON-LD block, `<script type="application/ld+json">` without
  `src`, whose content must parse as JSON. It is data for search and answer engines, not code:
  the browser does not run it and the CSP stays as it is. Every other script is refused.
- Absolute URLs appear only in two places: `https://` links, and the metadata for search and
  sharing (`<link rel="canonical">`, `og:*`, `twitter:*`), which may name only
  `https://clouddnagmbh.github.io/requesttailor/` (this site, on GitHub Pages). Plain `https://`
  links may go anywhere, for example to the web app (`https://app.requesttailor.com/`) and the
  separate product website (`https://requesttailor.com/en/`, linked from the home page and every
  footer). Stylesheets, images and scripts stay relative, except on `404.html`: GitHub Pages
  serves it at any depth, so it names its stylesheet, icon and logo by their absolute github.io
  URL (same origin, so the CSP's `'self'` allows it; the check requires them to be site files).
- `robots.txt` takes effect only at a host root. On `github.io/requesttailor/` it is advisory;
  submit `sitemap.xml` in Search Console instead.
- Every page except `404.html` has a title of at most 60 characters and a description of at most
  155 characters, both unique, a canonical link that equals its place in `sitemap.xml`, Open
  Graph and Twitter tags, an `og:image` that is a file of the site, and `alt`, `width` and
  `height` on every image. A new page also goes into `sitemap.xml` and, if it is important,
  into `llms.txt`.
- Links between pages are relative (`../docs/`), so the site works at a domain root and in a
  subfolder such as `https://<owner>.github.io/requesttailor/`.
- The performance budgets on the home page are marked `data-budget="<metric>"` and must equal
  `perf/budgets.toml`.
- Texts describe what the release contains. Planned work is labelled as planned. No pricing,
  no paid features, no sign-in.
- The name RequestTailor and its logos are not under the Apache license
  ([TRADEMARKS.md](../TRADEMARKS.md)); the footer says so. Pages that name SAP products carry
  the SAP trademark note.

`node scripts/check-website.mjs` checks all of this (and runs in `npm run test:scripts`
through `scripts/check-website.test.mjs`).

## Preview

```sh
python3 -m http.server 8000 --bind 127.0.0.1 --directory website
# http://127.0.0.1:8000/
```

## Publish

Only by hand: Actions > **Website** > Run workflow (`.github/workflows/website.yml`). See
[RELEASING.md](../RELEASING.md).
