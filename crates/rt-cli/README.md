# rt

The RequestTailor command line: `rt send` and the companion `rt agent` (below).

```sh
rt send <request-file> [--env NAME] [--var NAME=VALUE]... [--body-limit BYTES] [--pretty]
rt agent [--port PORT] [--allow-origin ORIGIN]...
rt --help
rt --version
```

`rt send` sends one request from an OpenCollection (YAML) collection, for example an
unchanged Bruno collection. The collection root is the nearest folder above `<request-file>`
that contains `opencollection.yml`.

| Option | Meaning |
| --- | --- |
| `-e`, `--env NAME` | Use the environment named `NAME` |
| `--var NAME=VALUE` | Runtime variable, highest precedence (repeatable). Use it for secrets and to point a collection at another host, e.g. `--var host=http://localhost:3000` |
| `--body-limit BYTES` | Keep at most this many response body bytes in the report (default 10 MiB); `size` still counts all bytes |
| `--pretty` | Indent the JSON |

## Output

Machine-readable first: `rt send` prints one JSON object, the send report, on stdout. It is
the same `SendReport` that the WASM core (`rt-wasm`) returns, so results are identical across
surfaces. Fields: `version` (report format, currently 1), `request_path`, `environment`,
`request` (what went on the wire, secret values shown as `***`), `response` (`status`,
`version`, `headers`, final `url`, `redirects`, `body` as `{"encoding": "utf8" | "base64",
"data": ...}`, `size`, `truncated`, `timing` and `total` in milliseconds), `variables`
(which variables were used and where they came from), `unresolved` (placeholders without a
value) and `error` (`{"kind": "not-found" | "prepare" | "transport", "message": ...}`).

The report is printed whenever the collection could be read, also on errors. Usage errors and
unreadable collections print a message to stderr and nothing to stdout. A human-readable
output mode is not part of wave 2; pipe through `jq` meanwhile.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | A response was received, whatever its HTTP status (a 404 or 500 is still 0) |
| 1 | Transport error, no response: connection, DNS, TLS, timeout, redirect limit |
| 2 | Usage error, no collection found, invalid files, or request not in the collection |
| 3 | The request could not be prepared: unknown environment, invalid URL after variable substitution, unsupported auth, unreadable body file |

## Files

Body files (file bodies, multipart file parts) are read relative to the collection root.
Absolute paths, `..` and symlinks that lead outside the collection folder are refused.

## Companion: `rt agent`

```sh
rt agent [--port PORT] [--allow-origin ORIGIN]...
```

The web app runs the RequestTailor core in the browser, so the browser's rules apply: no
request to a server without CORS headers, none of the forbidden headers, no client
certificates. `rt agent` is the companion for those requests. It runs on your machine,
receives a request the web app has already prepared (variables, auth and body resolved in
the browser), sends it with the same native transport as `rt send` and streams the answer
back. When the browser blocks a request, the web app offers the companion with one click.

| Option | Meaning |
| --- | --- |
| `--port PORT` | Listen on `127.0.0.1:PORT` (default 47812, where the web app looks) |
| `--allow-origin ORIGIN` | A web app origin (`scheme://host[:port]`) that may pair and send; repeatable. When given, it replaces the default list (`http://127.0.0.1:5173`, `http://localhost:5173`, the web app's dev server). A hosted web app needs its origin here, e.g. `--allow-origin https://app.example` |

On start, `rt agent` prints a pairing code and a pairing link for each allowed origin:

```text
Pairing code: 7KQ2-M9XD (single use, valid for 10 minutes)
To pair, open a pairing link in the browser:
  http://127.0.0.1:5173/#rt-pair=7KQ2M9XD
```

Open the link (or type the code where the web app asks for it). The web app exchanges the
code for a token and from then on uses the companion when you choose it. Stop the companion
with Ctrl+C; restarting it revokes every pairing.

### Security model

The companion is a local server that sends arbitrary requests with your machine's network
access, including internal hosts. Each of these rules is tested in `tests/agent.rs`.

1. **Loopback only.** It binds `127.0.0.1`, never `0.0.0.0`; other machines cannot reach it.
2. **Host check (DNS rebinding).** Every request needs `Host: 127.0.0.1:<port>` or
   `Host: localhost:<port>` and an origin-form target (`/v1/...`); anything else gets `403`.
   A web page on a rebound DNS name would send its own name as `Host`.
3. **Origin allowlist (CORS).** Requests with an `Origin` outside the allowlist get `403`
   without any CORS header, so the page cannot read even that. Allowed origins get
   `Access-Control-Allow-Origin: <origin>` (never `*`) and `Vary: Origin`. Preflights are
   answered only for the protocol's methods (`GET` status, `POST` pair and send) and headers
   (`Authorization`, `Content-Type`), and Chrome's Private Network Access preflight
   (`Access-Control-Request-Private-Network: true`) gets
   `Access-Control-Allow-Private-Network: true` only for allowed origins. A request
   without `Origin` (not from a web page, e.g. `curl`) may only read `GET /v1/status`.
4. **Pairing.** The code has 40 random bits (8 characters, Crockford base32), is valid once
   and for 10 minutes. After 5 wrong attempts it is replaced; every used, expired or replaced
   code is followed by a new one in the terminal. Pairing needs an allowed `Origin`. A
   successful pairing returns a 256-bit random token bound to that origin.
5. **Tokens.** `POST /v1/send` needs `Authorization: Bearer <token>`, and the token must
   belong to the request's `Origin`; otherwise `401` (with CORS headers, so the web app can
   offer to pair again). Tokens are compared in constant time and live in memory only:
   nothing is written to disk, and a restart revokes all of them.
6. **Nothing else.** Three endpoints, no file access, no configuration over HTTP. The
   companion refuses to send requests to itself. Requests are capped at 64 MiB (`413`), the
   pairing request at 4 KiB; the request head must arrive within 10 s.
7. **Logging.** One line per request on stderr with method and host only (`rt agent: GET
   api.internal:8443 -> 200 (35 ms to first byte)`), plus pairings and rejections. Paths,
   queries, headers and bodies are never logged.

What it does not protect against: other programs running as you on the same machine can
talk to `127.0.0.1` too, but they cannot pair without the code printed in your terminal. A
compromised page on an allowed origin (XSS) can use that origin's token like the web app
itself; allow only origins you trust.

### Protocol (version 1)

Types: `rt_core::transport::companion`.

| Endpoint | Request | Answer |
| --- | --- | --- |
| `GET /v1/status` | optional `Authorization: Bearer <token>` | `200 {"protocol": 1, "version": "...", "paired": bool}`; `paired` is true when the token belongs to the `Origin` |
| `POST /v1/pair` | `{"code": "7KQ2-M9XD"}` (dash, spaces and case do not matter) | `200 {"token": "..."}`, or `403 {"error": "wrong code" \| "code expired" \| ...}` |
| `POST /v1/send` | `Authorization: Bearer <token>`, JSON `WireRequest` (method, url, headers, base64 body, content type, timeout, redirect settings) | `200`, header `x-rt-head` with the JSON `WireHead` (status, version, headers, final URL, redirects, timing; non-ASCII escaped), body = the response body, streamed; `502` with a JSON transport error (`{"kind": "timeout"}`, `{"kind": "connect", "message": "..."}`, ...); `401` without a valid token |

The timeout and redirect settings of the `WireRequest` apply exactly as in `rt send`; the
timeout covers the whole exchange including the body. If it fires while the body is
streaming, the connection is cut and the web app reports a timeout.
