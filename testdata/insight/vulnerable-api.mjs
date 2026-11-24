// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19 fixture (strands R, S): a deliberately vulnerable synthetic API on loopback, for the
// passive and active security checks. Never expose it; it listens on 127.0.0.1 only.
//   node testdata/insight/vulnerable-api.mjs [port]      default 48800
// Each path shows one problem the audit must find, and /good shows none:
//   /cors      Access-Control-Allow-Origin: * with Allow-Credentials: true
//   /headers   no security headers, `Server: Apache/2.4.1`, `X-Powered-By: PHP/5.2`
//   /token     a JWT with alg none and no exp in the body
//   /cookie    Set-Cookie without Secure, HttpOnly, SameSite
//   /error     a stack trace in a 500 page
//   /nolimit   200 on every call, no rate-limit headers (500 calls in a row)
//   /good      security headers, a limited rate, nothing to find

import { createServer } from "node:http";

const port = Number(process.argv[2] ?? 48800);
const b64 = (o) => Buffer.from(JSON.stringify(o)).toString("base64url");
const jwtNone = `${b64({ alg: "none", typ: "JWT" })}.${b64({ sub: "user-1" })}.`;

createServer((req, res) => {
  const path = new URL(req.url, "http://localhost").pathname;
  const json = { "content-type": "application/json" };
  switch (path) {
    case "/cors":
      res.writeHead(200, { ...json, "access-control-allow-origin": "*", "access-control-allow-credentials": "true" });
      return res.end('{"ok":true}');
    case "/headers":
      res.writeHead(200, { ...json, server: "Apache/2.4.1", "x-powered-by": "PHP/5.2" });
      return res.end('{"ok":true}');
    case "/token":
      res.writeHead(200, json);
      return res.end(JSON.stringify({ access_token: jwtNone }));
    case "/cookie":
      res.writeHead(200, { ...json, "set-cookie": "sid=abc123; Path=/" });
      return res.end('{"ok":true}');
    case "/error":
      res.writeHead(500, { "content-type": "text/plain" });
      return res.end("Error: boom\n    at handler (/srv/app/orders.js:42:11)\n    at Layer.handle (/srv/app/node_modules/express/lib/router/layer.js:95:5)");
    case "/good":
      res.writeHead(200, {
        ...json,
        "strict-transport-security": "max-age=31536000",
        "x-content-type-options": "nosniff",
        "cache-control": "no-store",
        "content-security-policy": "default-src 'none'",
        "x-ratelimit-limit": "100",
        "x-ratelimit-remaining": "99",
      });
      return res.end('{"ok":true}');
    default:
      res.writeHead(path === "/nolimit" ? 200 : 404, json);
      return res.end("{}");
  }
}).listen(port, "127.0.0.1", () => console.error(`vulnerable-api on http://127.0.0.1:${port}`));
