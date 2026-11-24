// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19 fixture (strand A): a fake tracing backend (Jaeger-style JSON) on loopback.
//   node testdata/insight/fake-trace-host.mjs [port]      default 48782
// GET /api/traces/:traceId answers one trace with three spans (gateway, orders, db) whose
// start times and durations the strand's tests check; unknown ids answer 404.

import { createServer } from "node:http";

const port = Number(process.argv[2] ?? 48782);
const KNOWN = "4bf92f3577b34da6a3ce929d0e0e4736";

createServer((req, res) => {
  const m = new URL(req.url, "http://localhost").pathname.match(/^\/api\/traces\/([0-9a-f]{32})$/);
  if (!m || m[1] !== KNOWN) {
    res.writeHead(404, { "content-type": "application/json" });
    return res.end('{"errors":[{"msg":"trace not found"}]}');
  }
  const span = (id, parent, op, svc, start, dur) => ({
    traceID: KNOWN, spanID: id, operationName: op, processID: svc,
    references: parent ? [{ refType: "CHILD_OF", traceID: KNOWN, spanID: parent }] : [],
    startTime: 1_900_000_000_000_000 + start * 1000, duration: dur * 1000, tags: [],
  });
  res.writeHead(200, { "content-type": "application/json" });
  res.end(JSON.stringify({ data: [{
    traceID: KNOWN,
    spans: [span("aaaaaaaaaaaaaaa1", null, "GET /orders", "gateway", 0, 120), span("aaaaaaaaaaaaaaa2", "aaaaaaaaaaaaaaa1", "list", "orders", 10, 100), span("aaaaaaaaaaaaaaa3", "aaaaaaaaaaaaaaa2", "select", "db", 20, 70)],
    processes: { gateway: { serviceName: "gateway" }, orders: { serviceName: "orders" }, db: { serviceName: "db" } },
  }] }));
}).listen(port, "127.0.0.1", () => console.error(`fake-trace-host on http://127.0.0.1:${port}`));
