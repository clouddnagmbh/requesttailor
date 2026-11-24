// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19 fixture (strands C, D, E, F): a small synthetic orders API on loopback.
//   node testdata/insight/orders-api.mjs [port]      default 48700
// - POST /orders -> 201 + Location + ETag; GET/PUT/DELETE /orders/:id (If-Match -> 412);
// - users alice and bob (header `x-user`): bob reading alice's order is allowed on purpose
//   (broken object-level authorisation), DELETE /admin/orders needs `x-user: admin` but is
//   also reachable as bob (broken function-level authorisation);
// - POST /boom with a body that has `"poison": true` answers 500 (for the shrinker);
// - GET /health. Everything in memory, nothing real.

import { createServer } from "node:http";

const port = Number(process.argv[2] ?? 48700);
const orders = new Map();
let next = 1;

const send = (res, status, body, headers = {}) => {
  const text = body === undefined ? "" : JSON.stringify(body);
  res.writeHead(status, { "content-type": "application/json", ...headers });
  res.end(text);
};

createServer((req, res) => {
  const url = new URL(req.url, "http://localhost");
  let raw = "";
  req.on("data", (c) => (raw += c));
  req.on("end", () => {
    const user = req.headers["x-user"] ?? "anonymous";
    const body = raw ? JSON.parse(raw) : undefined;
    const m = url.pathname.match(/^\/orders\/(\d+)$/);
    if (url.pathname === "/health") return send(res, 200, { ok: true });
    if (url.pathname === "/orders" && req.method === "POST") {
      const id = next++;
      orders.set(id, { id, owner: user, ...body, version: 1 });
      return send(res, 201, orders.get(id), { location: `/orders/${id}`, etag: '"1"' });
    }
    if (m && orders.has(Number(m[1]))) {
      const order = orders.get(Number(m[1]));
      if (req.method === "GET") return send(res, 200, order, { etag: `"${order.version}"` });
      if (req.method === "PUT") {
        if (req.headers["if-match"] && req.headers["if-match"] !== `"${order.version}"`) return send(res, 412, { error: "stale" });
        Object.assign(order, body, { version: order.version + 1 });
        return send(res, 200, order, { etag: `"${order.version}"` });
      }
      if (req.method === "DELETE") return orders.delete(order.id), send(res, 204);
    }
    if (url.pathname === "/admin/orders" && req.method === "DELETE") return orders.clear(), send(res, 204);
    if (url.pathname === "/boom" && body?.poison === true) return send(res, 500, { error: "boom" });
    if (url.pathname === "/boom") return send(res, 200, { ok: true });
    send(res, 404, { error: "not found" });
  });
}).listen(port, "127.0.0.1", () => console.error(`orders-api on http://127.0.0.1:${port}`));
