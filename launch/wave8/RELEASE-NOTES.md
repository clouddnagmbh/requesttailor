<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

<!-- Draft for the GitHub release of launch 2 (wave 8). Check every line against the merged
     main branch before publishing (CHECKLIST.md, step 1). -->

RequestTailor speaks more than REST now, and AI agents can use it safely. This release adds
gRPC, WebSocket, Server-Sent Events and GraphQL, an MCP server that lets coding agents run
your collections with approvals for writes to production, an MCP client, an optional AI
assistant with your own key, and three tools for the command line: a mock server, a
documentation generator and a reproducible benchmark. Still no account, no cloud, no
telemetry.

## Highlights

### Protocols beyond HTTP

- **gRPC**: unary and streaming calls from `.proto` files or server reflection, messages
  as JSON, metadata as headers. Request files stay OpenCollection `grpc` items.
- **WebSocket**: connect, send saved messages, see every frame in a session log.
- **Server-Sent Events**: an HTTP request whose `text/event-stream` response is shown as a
  live list of events.
- **GraphQL**: a request body with query and variables, sent as JSON.
- Every session runs on the same core on the desktop and, through the `rt agent` companion,
  in the web app. Secret values are masked in session logs as everywhere else.

### Agents

- **MCP server: `rt mcp`.** A coding agent (any MCP client over stdio) can list the requests
  and environments of a collection, read a request, send it and run a folder, with the same
  core and the same reports as the app and `rt run`.
- **Writes to production wait for you.** Each environment says what agents may do (nothing,
  read, write) and whether it is production. By default agents only read. A writing request
  against a production environment waits until you approve it in the app or with
  `rt approve <id>` (`rt approvals` lists what is waiting). Every agent call is written to a
  local audit log.
- **Agents never see secret values**: reports are masked, secrets appear only by name. This
  is covered by an automated test of the whole path.
- **MCP client**: connect to MCP servers (stdio on the desktop and CLI, Streamable HTTP
  everywhere), call their tools and save the calls as tests next to the collection;
  `rt mcp-test` runs them in a pipeline.
- **AI assistant, off until you configure it**: bring your own Anthropic key or point it at
  an OpenAI-compatible endpoint such as a local model. Secret values are removed from
  everything before it is sent; answers are suggestions you review, never saved on their own.
- **Secrets from the keychain in the terminal**: `rt send`, `rt run`, `rt mcp` and session
  requests read secret variables from the OS keychain where the desktop app keeps them, so
  nothing has to be passed with `--var` on a developer machine.

### Command line

- **`rt mock`** serves the examples saved with your requests (OpenCollection `examples`,
  also brought over by the Postman import): method and path pick the route, query, headers
  and body pick among several examples, anything else gets a 404 with the list of routes.
  Loopback by default, CORS for the web app.
- **`rt docs`** writes a static HTML page of a collection: folders, requests, parameters,
  headers, bodies, auth type, examples. No scripts, nothing loaded from elsewhere, light and
  dark mode. Secret values never appear.
- **Reproducible benchmark**: `node scripts/bench/run.mjs` measures RequestTailor and the
  other API clients installed on the same machine the same way: start to first window, idle
  memory, and RequestTailor's own budgets. Results with machine and versions are in
  `bench/results/`; the website's benchmark page shows the latest.

### Run in RequestTailor

A badge for your README that points to a page explaining how to open your repository's
collection on the desktop, in a pipeline and in the web app.

## Downloads

Unchanged from the first release; see the website's download page for what is published.
Windows and Linux: use the web app or the CLI; there are no desktop installers for them.

Verify with `SHA256SUMS` and its signature `SHA256SUMS.asc`, and compare the fingerprint with
the one on the website's download page.

## Not in this release

- A hosted web app; the web app runs from the repository.
- Building plugins from TypeScript; plugins are written in Rust for now.
- Automatic updates: the app does not check for updates. Watch the repository's releases.

Postman, Insomnia, Bruno and Hoppscotch are trademarks of their respective owners.
