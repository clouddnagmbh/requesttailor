<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# Website

The project website: plain HTML and one stylesheet, no build step, no framework.

| Path | Page |
| --- | --- |
| `index.html` | What RequestTailor is: principles, performance budgets, surfaces |
| `sap/` | The OData plugin for S/4HANA, CAP and RAP |
| `download/` | Release files per OS, checksum and signature verification |
| `docs/` | Getting started, collections, environments and secrets, CLI, MCP, plugins, OData guide, privacy |
| `assets/site.css` | The only stylesheet; light and dark follow `prefers-color-scheme` |

## Rules

- Nothing is loaded from other servers and nothing runs: no scripts, no web fonts, no
  analytics, no cookies, no embeds (product principle 2). Every page has a
  `Content-Security-Policy` meta tag with `default-src 'none'` that enforces this in the
  browser.
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
