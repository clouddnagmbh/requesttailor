<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# Launch 2 checklist (wave 8)

Only the project owner publishes. Everything in this folder is a draft; nothing has been
posted, tagged or released. Tick each box when it is done. Order matters: the posts link to
the release and the website, so those come first.

## 1. Before anything is public

- [ ] **Wave 8 is merged and green**: `ci-ok` on main (or, while CI cannot run, the local
      gate commands from CLAUDE.md), and the wave 8 gate demonstrated by hand: an agent runs
      a collection over MCP and a writing request against PRD waits for approval; gRPC,
      WebSocket and SSE on the desktop and through the companion in the web app.
- [ ] **Facts in the drafts match main.** Read [RELEASE-NOTES.md](RELEASE-NOTES.md),
      [BLOG-POST.md](BLOG-POST.md) and [POSTS.md](POSTS.md) against what was merged (the
      drafts were written while strands A to E were still in progress): gRPC call types and
      reflection, SSE view, GraphQL, MCP tool names (`crates/rt-cli/tests/gate_w8.rs`), the
      approval commands, the MCP client and `rt mcp-test`, the AI providers. Remove what did
      not land.
- [ ] **Fresh benchmark on a quiet machine.** Quit other apps and builds, unlock the screen,
      then `npm run build -w apps/desktop && node scripts/bench/run.mjs --browser`. Check the
      load average in the new `bench/results/<date>-macos.md` (it should be well below the
      number of CPUs), commit the JSON, the Markdown and the updated
      `website/benchmark/index.html`, and copy the numbers into the blog post and the posts
      (`BENCHMARK_SUMMARY`, `<numbers>`). If a budget is missed, fix that before launching,
      not the text.
- [ ] **Desktop wording.** The website's download page is the reference, and the drafts only
      point to it ("get it from the download page"): the macOS app and `rt` from the GitHub
      releases page, Windows and Linux through the web app and the CLI. Apple distribution
      (company enrollment, Developer ID, notarization) is pending: until a signed macOS build
      is on the releases page, do not link a `.dmg` or promise a notarized download in any
      post, and update the download page's note if it changes.
- [ ] **Site address in the badge snippets.** `website/run/index.html` uses
      `https://clouddnagmbh.github.io/requesttailor/` (the GitHub Pages default). With a
      custom domain, replace it there and in the posts.
- [ ] Optional: decide whether `TRADEMARKS.md` should say explicitly that the unchanged
      "Run in RequestTailor" badge may be used in READMEs.

## 2. Release

- [ ] Version bump (`[workspace.package] version`, `tauri.conf.json`), release notes from
      [RELEASE-NOTES.md](RELEASE-NOTES.md), signed tag, draft release, files checked on clean
      machines, publish (as in [../CHECKLIST.md](../CHECKLIST.md#release)).
- [ ] Publish the website (Actions > Website > Run workflow). Open `/`, `/benchmark/`,
      `/run/`, `/docs/#mock`, `/docs/#api-docs` in light and dark mode and on a phone; check
      that the badge image loads on `/run/`.

## 3. Posts, in this order

| When | Where | Text | Link to |
| --- | --- | --- | --- |
| Day 1, morning | GitHub release | RELEASE-NOTES.md | |
| Day 1, morning | Project blog or dev.to | BLOG-POST.md | release, `/benchmark/` |
| Day 1, after the blog | Mastodon / Bluesky | POSTS.md "Short" | blog post |
| Day 1, after the blog | LinkedIn | POSTS.md "LinkedIn" | blog post |
| Day 1, US morning | Hacker News (Show HN) | POSTS.md "Show HN" | repository |
| Day 2 | Reddit (r/programming or r/webdev; read each community's self-promotion rules first) | POSTS.md "Reddit" | blog post |
| Day 2 | SAP Community (only if the OData plugin changed) | adapt `../SAP-COMMUNITY-POST.md` | release |

- [ ] Be around for the first hours after Show HN and Reddit to answer questions; the
      benchmark method is the most likely topic (point to `bench/README.md`, "Limits").
- [ ] Do not name competitors in a negative way; the benchmark table speaks for itself and
      the method page says what is not compared.

## 4. After the launch

- [ ] Collect questions and issues that came up, and file them for wave 9.
- [ ] If people report benchmark results from other machines, link their result files rather
      than mixing numbers from different machines in one table.
