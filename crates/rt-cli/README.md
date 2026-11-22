# rt

The RequestTailor command line: `rt send`, `rt import`, `rt scan` and the companion `rt agent`
(below).

```sh
rt send <request-file> [--env NAME] [--var NAME=VALUE]... [--body-limit BYTES] [--pretty]
rt import <file>... --out DIR [--format FORMAT] [--json]
rt scan [PATH]... [--json]
rt agent [--port PORT] [--allow-origin ORIGIN]... [--allow-secrets]
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

## External secrets

An environment can read some variables from a secret manager instead of the keychain
(`externalSecrets` in the environment file; the file holds only the reference):

```yaml
name: Staging
externalSecrets:
  type: one-password
  variables:
    - name: apiToken
      secretReference: op://Staging/api/token
```

`rt send --env Staging` then runs the manager's own CLI with your existing login, and the
value becomes a secret variable of the environment (shown as `***` in the report):

| `type` | Key of the reference | Command |
| --- | --- | --- |
| `one-password` | `secretReference: op://vault/item/field` | `op read --no-newline REF` |
| `hashicorp-vault-server`, `hashicorp-vault-cloud` | `path: secret/app#field` (field defaults to the variable name) | `vault kv get -format=json PATH`; `vault read -format=json PATH` for `mount/data/...` |
| `aws-secrets-manager` | `secretName: ID` (or ARN), `ID#key` for one key of a JSON secret | `aws secretsmanager get-secret-value --secret-id=ID --query SecretString --output text` |
| `azure-key-vault` | `vaultName: VAULT`, `VAULT/SECRET` or `VAULT/SECRET/VERSION` (secret defaults to the variable name) | `az keyvault secret show --vault-name=VAULT --name=SECRET --query value --output tsv` |
| `gcp-secrets-manager` | `projectId: PROJECT`, `PROJECT/SECRET[/VERSION]` or `projects/PROJECT/secrets/SECRET[/versions/VERSION]` (secret defaults to the variable name, version to `latest`) | `gcloud secrets versions access VERSION --secret=SECRET --project=PROJECT` |

The CLI runs without a shell, without input and with a 60 s timeout; references that look
like options are refused. A missing CLI, a missing login or a timeout stops the send with
exit code 3 and a message that says what to install or which login command to run. A
variable given with `--var` is not read from the manager, so a pipeline can pass the value
itself.

## Files

Body files (file bodies, multipart file parts) are read relative to the collection root.
Absolute paths, `..` and symlinks that lead outside the collection folder are refused.

## Import: `rt import`

`rt import` converts exports of other tools into a new collection folder and prints what could
not be carried over exactly (the loss report).

```sh
rt import shop.postman_collection.json prod.postman_environment.json --out ./shop
rt import api.yaml --format openapi --out ./api --json
```

| Option | Meaning |
| --- | --- |
| `-o`, `--out DIR` | Folder to create; it must not exist or must be empty |
| `-f`, `--format FORMAT` | `postman` (v2.0, v2.1), `insomnia` (v4, v5), `openapi` (3.0 to 3.2), `curl`, `har` (1.2) or `http` (VS Code REST Client, JetBrains); default: detected per file |
| `--json` | Print `{ collectionName, out, files, report }` instead of text |

Several files form one collection: a Postman collection with its environment and globals
files; `.http` files (one folder each) with `http-client.env.json` and
`http-client.private.env.json`; several cURL files (one folder each).

The report lists every loss with its location (`Users / Get user`), kind, effect (`dropped`:
not in the collection; `approximated`: in it, but not exactly) and one sentence. Scripts and
tests are reported with their line counts. Secret values (Postman `secret` variables, Insomnia
private environments, the JetBrains private env file) are never written: the variables are
created as secrets without a value; set them in Environments. Credentials typed into requests
stay as in the source and each gets a report line suggesting a secret variable.

Exit code 0 once the collection is written, even if the report lists losses; 2 if the files
cannot be read or imported, or `--out` is in use (nothing is written then).

## Secret scanner: `rt scan`

```sh
rt scan [PATH]... [--json]
```

Finds credentials typed into collection files before they reach Git: known token formats
(GitHub, GitLab, Slack, Stripe, `sk-`, AWS key ids, Google API keys, npm, JWTs), private keys,
`Bearer` and `Basic` credentials, passwords in URLs, and values of credential-like fields
(passwords, client secrets, API-key headers, `token`, secret variables with a value). Paths
are files or folders (default: the current folder); folders are scanned recursively without
`.git` and `node_modules`, and binary files, files that are not UTF-8 and files over 4 MiB are
skipped. The desktop and web apps run the same scanner before every commit and refuse it
while there are findings.

Not findings: values that are only `{{variables}}` (or mix text and variables), masks such as
`***`, environment references (`$TOKEN`, `${TOKEN}`, `process.env.TOKEN`) and placeholders
(`<token>`, `your-token`, `xxx`). A line that contains `rt-scan:allow` is skipped, e.g. a
YAML comment `# rt-scan:allow`; `rt-scan:allow-next-line` on its own line skips the next one.

Output: one block per finding with file, line, kind, a masked snippet and a hint (the value
is never printed), then a summary. `--json` prints
`{"scanned": N, "findings": [{"path", "line", "kind", "snippet", "hint"}]}`. Exit code 0 when
clean, 1 with findings, 2 if a path cannot be read.

```text
users/me.yml:10: token
    token: ***
    Make it a secret variable: in Environments add `token`, mark it as secret (the value is kept in the OS keychain, or comes from a secret manager under External secrets) and write `{{token}}` here instead. Not a credential? Add `# rt-scan:allow` to the line.

1 credential in 1 file (3 files scanned). Move them into secret variables, or mark a line that is not a credential with `# rt-scan:allow`.
```

As a Git pre-commit hook (`.git/hooks/pre-commit`, executable), scanning the staged files:

```sh
#!/bin/sh
# Refuse commits that add credentials to collection files.
files=$(git diff --cached --name-only --diff-filter=ACMR -- '*.yml' '*.yaml' '*.json')
[ -z "$files" ] && exit 0
# shellcheck disable=SC2086
exec rt scan $files
```

The hook scans the files in the working tree; stage them as they are before committing. In
a pipeline, `rt scan path/to/collection` fails the job on findings.

## Companion: `rt agent`

```sh
rt agent [--port PORT] [--allow-origin ORIGIN]... [--allow-secrets]
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
| `--allow-secrets` | Also read external secrets for the web app (see below). Off by default |
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

**External secrets.** The browser cannot run `op`, `vault` and the like. With
`--allow-secrets`, a paired web app sends the references of an environment's external secrets
(`POST /v1/secrets`), and the companion reads them with the CLIs exactly as `rt send` does
(above) and returns the values. The web app keeps them in memory for the session, only to
send; they are masked in reports and never stored. Without the flag, or without the
companion, the web app says how to turn it on.

### Security model

The companion is a local server that sends arbitrary requests with your machine's network
access, including internal hosts. Each of these rules is tested in `tests/agent.rs`
(`tests/external_secrets.rs` for the secrets endpoint).

1. **Loopback only.** It binds `127.0.0.1`, never `0.0.0.0`; other machines cannot reach it.
2. **Host check (DNS rebinding).** Every request needs `Host: 127.0.0.1:<port>` or
   `Host: localhost:<port>` and an origin-form target (`/v1/...`); anything else gets `403`.
   A web page on a rebound DNS name would send its own name as `Host`.
3. **Origin allowlist (CORS).** Requests with an `Origin` outside the allowlist get `403`
   without any CORS header, so the page cannot read even that. Allowed origins get
   `Access-Control-Allow-Origin: <origin>` (never `*`) and `Vary: Origin`. Preflights are
   answered only for the protocol's methods (`GET` status, `POST` pair, send and secrets) and headers
   (`Authorization`, `Content-Type`), and Chrome's Private Network Access preflight
   (`Access-Control-Request-Private-Network: true`) gets
   `Access-Control-Allow-Private-Network: true` only for allowed origins. A request
   without `Origin` (not from a web page, e.g. `curl`) may only read `GET /v1/status`.
4. **Pairing.** The code has 40 random bits (8 characters, Crockford base32), is valid once
   and for 10 minutes. After 5 wrong attempts it is replaced; every used, expired or replaced
   code is followed by a new one in the terminal. Pairing needs an allowed `Origin`. A
   successful pairing returns a 256-bit random token bound to that origin.
5. **Tokens.** `POST /v1/send` and `POST /v1/secrets` need `Authorization: Bearer <token>`, and the token must
   belong to the request's `Origin`; otherwise `401` (with CORS headers, so the web app can
   offer to pair again). Tokens are compared in constant time and live in memory only:
   nothing is written to disk, and a restart revokes all of them.
6. **Secrets only when asked for.** `POST /v1/secrets` answers `403` unless the companion
   was started with `--allow-secrets` (the start banner says so), and like send it needs an
   allowed `Origin` and that origin's token. It runs only the five managers' CLIs, with
   fixed arguments built from the references (no shell; references that start with `-` or
   contain whitespace are refused), a 60 s timeout and your existing logins; it keeps no
   values (no cache, nothing on disk). Answers carry `Cache-Control: no-store`. A page on an
   allowed origin with a token can read any secret your CLI logins can, so enable the flag
   only for origins you trust as much as your terminal.
7. **Nothing else.** Four endpoints, no file access, no configuration over HTTP. The
   companion refuses to send requests to itself. Requests are capped at 64 MiB (`413`), the
   pairing request at 4 KiB, the secrets request at 64 KiB; the request head must arrive
   within 10 s.
8. **Logging.** One line per request on stderr with method and host only (`rt agent: GET
   api.internal:8443 -> 200 (35 ms to first byte)`), plus pairings and rejections. Paths,
   queries, headers and bodies are never logged. For secrets, only the provider, the number
   of values and the origin (`rt agent: read 2 secret(s) from 1Password for
   http://127.0.0.1:5173`); never references or values.

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
| `POST /v1/secrets` | `Authorization: Bearer <token>`, JSON `SecretsRequest` (`{"provider": "one-password", "variables": [{"name": "...", "reference": "...", "enabled": true}]}`) | `200 {"values": [["name", "value"], ...]}` for the enabled entries; `422 {"error": "..."}` when a secret cannot be read (CLI missing, not signed in, timeout); `403 {"error": "..."}` without `--allow-secrets`; `401` without a valid token |
| `POST /v1/send` | `Authorization: Bearer <token>`, JSON `WireRequest` (method, url, headers, base64 body, content type, timeout, redirect settings) | `200`, header `x-rt-head` with the JSON `WireHead` (status, version, headers, final URL, redirects, timing; non-ASCII escaped), body = the response body, streamed; `502` with a JSON transport error (`{"kind": "timeout"}`, `{"kind": "connect", "message": "..."}`, ...); `401` without a valid token |

The timeout and redirect settings of the `WireRequest` apply exactly as in `rt send`; the
timeout covers the whole exchange including the body. If it fires while the body is
streaming, the connection is cut and the web app reports a timeout.
