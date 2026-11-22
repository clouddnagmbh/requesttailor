<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

<!-- Drafts. Published only by the project owner, in the order of CHECKLIST.md. Replace
     <link to blog post>, <repository> and <numbers> (from bench/results/) first. -->

# Short posts for launch 2

## Short (Mastodon, Bluesky; under 300 characters)

RequestTailor 2 is out: gRPC, WebSocket, SSE and GraphQL in the same Git-friendly
collection files, an MCP server where writes to production wait for your approval, and a
benchmark you can run yourself. Open source, no account. <link to blog post>

## LinkedIn

RequestTailor 2 is out. It is an open-source API client whose requests are plain files in
your Git repository, with no account and no cloud.

New in this release:
- gRPC, WebSocket, Server-Sent Events and GraphQL next to your REST requests
- An MCP server for coding agents: each environment says what agents may do, writes to
  production wait for a person, and agents never see secret values
- An MCP client and an optional AI assistant that works with your own key or a local model
- `rt mock` serves the examples saved with your requests; `rt docs` writes a static
  documentation page
- A reproducible benchmark against other API clients on the same machine: <numbers>

Everything is Apache 2.0. <link to blog post>

## Show HN

**Title:** Show HN: RequestTailor 2 – API client with files in Git, MCP with approvals, and a
reproducible benchmark

**Text:**

RequestTailor is an open-source (Apache 2.0) API client with a Rust core. Requests are
OpenCollection YAML files in your repository (Bruno's format; files round-trip byte for
byte), the same core runs in a Tauri desktop app, in the browser as WebAssembly, and as the
`rt` CLI in CI. No account, no telemetry.

This release adds gRPC, WebSocket, SSE and GraphQL, and an MCP server (`rt mcp`) so coding
agents can run collections. Each environment declares what agents may do; writing requests
against production block until a human approves them (`rt approve` or a banner in the app),
everything goes to a local audit log, and secret values never reach the agent: there is a
test for exactly that.

We also added a benchmark script that measures us and whatever other API clients you have
installed, the same way on the same machine (spawn to first window, idle memory with all
helper processes, fresh profiles, no network for their web content). On our machine:
<numbers>. The method and its limits are in bench/README.md; we would like to hear where it
is unfair.

Repository: <repository>

## Reddit

**Title:** RequestTailor 2: open-source API client (files in Git) with gRPC, WebSocket, SSE,
an MCP server with approvals, and a benchmark script you can run against your installed
clients

**Text:**

Hi! RequestTailor keeps each request as a YAML file in your repo (OpenCollection, the format
Bruno writes), so reviews and diffs work in any Git tool, and the same Rust core runs on the
desktop, in the browser and in CI. No account.

What's new: gRPC/WebSocket/SSE/GraphQL; an MCP server for coding agents where writes to
production wait for your approval and secrets are never shown to the agent; `rt mock` and
`rt docs`; and a benchmark script that measures start-up and idle memory of RequestTailor and
any other API client you have installed, the same way (<numbers> on our machine; method in
bench/README.md).

Feedback, especially on the benchmark method, is very welcome. <link to blog post>
