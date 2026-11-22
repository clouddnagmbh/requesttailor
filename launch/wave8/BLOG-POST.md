<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

<!-- Draft. Published only by the project owner (CHECKLIST.md). Numbers come from
     bench/results/; re-check them against the latest result before publishing. -->

# RequestTailor 2: more protocols, agents that ask before they write, and numbers you can check

When we published RequestTailor, the promise was simple: your API requests are plain files in
your Git repository, the app is only a view on that folder, and nothing needs an account. The
second release keeps that promise and widens what fits into those files.

## More than REST

APIs are not only request and response. This release adds **gRPC** (unary and streaming,
from `.proto` files or server reflection), **WebSocket** sessions, **Server-Sent Events** and
**GraphQL**. They live in the same collection, as OpenCollection files next to your HTTP
requests, and run on the same Rust core on the desktop and in the browser. In the web app,
long-lived connections go through `rt agent`, the companion on your own machine, so the
browser's limits do not decide what you can test.

## Agents, with a hand on the brake

Coding agents are good at calling APIs and bad at knowing which calls are dangerous. `rt mcp`
gives an agent your collection as tools: list requests, read one, send it, run a folder. The
reports are the same as in the app and in `rt run`.

Three rules make that safe enough to use on real systems:

1. **Each environment says what agents may do**: nothing, read, or write. The default is read.
2. **A writing request against production waits for a person.** The app shows a banner, or
   you answer with `rt approve` in a terminal. Every call goes into a local audit log.
3. **Agents never see secret values.** Tokens and passwords are masked in every report and
   only named, never shown. An automated test checks the whole path, including what would go
   to a model.

The same care went into the optional **AI assistant**: it is off until you configure it with
your own key (or a local, OpenAI-compatible model), and every secret value is removed before
anything leaves your machine. Its answers are suggestions; nothing is saved without you.

RequestTailor also works the other way round: as an **MCP client** it connects to MCP
servers, calls their tools and keeps those calls as tests in the repository, so `rt mcp-test`
can check an MCP server in CI.

## Three small tools for the command line

- **`rt mock`** turns the examples saved with your requests into a server. A front end can
  work before the backend exists, and a test can run without a network.
- **`rt docs`** writes a static documentation page from the collection: one HTML file and a
  stylesheet, no scripts, nothing loaded from elsewhere, no secret values. Publish it with
  GitHub Pages next to the code it documents.
- **"Run in RequestTailor"**: a badge for your README that explains how to open the
  repository's collection on the desktop, in a pipeline and in the web app. No hosted service
  behind it, just a static page.

## Numbers you can check

Performance is a product principle with a budget: start in under 500 ms, under 150 MB of
memory when idle, smooth scrolling through a 100 MB response, a request sent in at most two
keystrokes after start. Every build measures these, and a missed budget fails the build.

"Fast" is easy to claim, so this release adds a **benchmark you can run yourself**. One
script measures RequestTailor and the other API clients installed on the same machine, the
same way: from starting the process to the first window, and the memory of the app with all
its helper processes when idle. Other apps get a fresh profile and no network for their web
content, so no account and no server is involved.

BENCHMARK_SUMMARY

The script, the definitions and every raw sample are in the repository (`bench/`), with the
machine and the versions. If your machine gives different numbers, the JSON says why.

## Still the same rules

No account, ever. No telemetry. Secrets stay in your keychain. What is free stays free. The
code is Apache 2.0.

Get it from the download page, or run the web app from the repository. Feedback and
contributions are welcome on GitHub.
