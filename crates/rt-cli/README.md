# rt

The RequestTailor command line: `rt send`, `rt run`, `rt import`, `rt scan`, Git for APIs
(`rt diff`, `rt merge-driver`, `rt bisect`, `rt changelog`), the MCP server `rt mcp`, MCP tests
and the demo server (`rt mcp-test`, `rt mcp-demo`) and the companion `rt agent` (below).

```sh
rt send <request-file> [--env NAME] [--var NAME=VALUE]... [--body-limit BYTES] [--pretty]
rt run [FOLDER] [--env NAME] [--var NAME=VALUE]... [--data FILE] [--reporter NAME[=FILE]]...
rt import <file>... --out DIR [--format FORMAT] [--json]
rt import openapi <file|url>... --out DIR [--names operation-id] [--group tag|path|flat] [--keep-spec] [--json]
rt scan [PATH]... [--json]
rt diff [<rev> | <rev>..<rev>] [--json] [<collection>]
rt merge-driver <base> <ours> <theirs> [<path>]
rt bisect --good <rev> --bad <rev> [--env NAME] [--json] [<folder>]
rt changelog <rev>..<rev> [<collection>] [--out FILE]
rt mcp [<collection>] [--var NAME=VALUE]... [--env-default NAME]
rt mcp-test [<path>...] [--env NAME] [--var NAME=VALUE]...
rt mcp-demo [--http] [--port PORT]
rt approvals [--json] | rt approve <id> | rt deny <id>
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
| `--plugin FILE.wasm` | Also run this plugin (repeatable) |
| `--no-plugins` | Do not run the installed plugins |
| `--cookie-jar FILE` | Send cookies from `FILE` and store new ones in it |
| `--no-prompt` | Fail instead of asking for an OAuth 2.0 login (pipelines) |
| `--no-scripts` | Do not run the collection's scripts; declarative assertions still run |
| `--sse` | Read an HTTP request's response as Server-Sent Events (a session, see below) |
| `--message TITLE` | Sessions: send this message variant instead of the selected one |
| `--duration SECONDS` | Sessions: keep the session open this long |
| `--until-close` | Sessions: keep the session open until the server ends it |

### Sessions: WebSocket, gRPC and SSE

WebSocket and gRPC requests, and HTTP requests with `--sse`, are sessions (wave 8). `rt send`
opens the session with the same preparation as a send (variables, auth including OAuth 2.0,
plugins, cookies, `before-request` scripts), sends the selected message (or `--message`) once
the session is open, and by default closes it after the first received message or after
10 s. `--duration` and `--until-close` change that; the later option wins.

The output is the masked message history (`StreamLog`): `version`, `request_path`,
`environment`, `kind` (`websocket`, `sse` or `grpc`), the masked `url`, `events` (`opened`
with the handshake or response head, `message` with `direction` `sent`/`received`, a `frame`
`{"type": "text" | "binary", "data": ...}` (binary as base64) and for SSE `name` and `id`,
`closed` with `code`, `reason` and `by_client`, or `error`), `notes`, and `error` (as in the
send report) when the session could not be opened. Secret values are `***` everywhere,
also in received messages. Exit codes as for a send: 0 when the session opened (whatever
the close code), 1 for a transport error (also an `error` event during the session), 2 when
the request or the `--message` variant is not found, 3 when the request could not be
prepared.

## Output

Machine-readable first: `rt send` prints one JSON object, the send report, on stdout. It is
the same `SendReport` that the WASM core (`rt-wasm`) returns, so results are identical across
surfaces. Fields: `version` (report format, currently 1), `request_path`, `environment`,
`request` (what went on the wire, secret values shown as `***`), `response` (`status`,
`version`, `headers`, final `url`, `redirects`, `body` as `{"encoding": "utf8" | "base64",
"data": ...}`, `size`, `truncated`, `timing` and `total` in milliseconds), `variables`
(which variables were used and where they came from), `unresolved` (placeholders without a
value) and `error` (`{"kind": "not-found" | "prepare" | "auth" | "plugin" | "script" |
"transport", "message": ...}`). Since wave 7, also `tests`, `assertions`, `logs`,
`var_changes` and `flow` from scripts and assertions (omitted when empty).

The report is printed whenever the collection could be read, also on errors. Usage errors and
unreadable collections print a message to stderr and nothing to stdout. A human-readable
output mode is not part of wave 2; pipe through `jq` meanwhile.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | A response was received, whatever its HTTP status (a 404 or 500 is still 0) |
| 1 | Transport error, no response: connection, DNS, TLS, timeout, redirect limit |
| 2 | Usage error, no collection found, invalid files, or request not in the collection |
| 3 | The request could not be prepared: unknown environment, invalid URL after variable substitution, unsupported auth, unreadable body file; authentication failed; a plugin or a pre-request script stopped it |
| 4 | A response was received but a test or assertion failed |
| 5 | A policy of this installation refused the request; nothing was sent (builds of this repository have none; see "Send guard") |

## Secret variables

`rt send`, `rt run` and `rt mcp` read the value of a secret variable from the OS keychain,
where the desktop app keeps it for this collection and environment, so a request sends from
the command line exactly as it does in the app. `--var` wins over the keychain, the keychain
over a secret manager. Without a keychain (a CI runner) the value stays unset; pass it with
`--var` or read it from a secret manager.

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

## Collection runs: `rt run`

`rt run` sends every request of a collection, or of one folder of it, in order, and reports
tests and assertions: the command for pipelines. It uses the same core, scripts, plugins and
auth as the desktop and web apps, so what is green in the app is green in CI.

```sh
rt run                                   # the collection (or folder) you are in
rt run api/users -e Staging --var token=$API_TOKEN
rt run . --data users.csv --reporter junit=reports/junit.xml --reporter json=reports/run.json
```

`FOLDER` is the collection root or a folder inside it (default: the current folder); a
folder runs only its requests. Order: folders and requests by `seq`, then name, depth first.
Variables that scripts set carry over to the following requests of the run (never into
files).

| Option | Meaning |
| --- | --- |
| `-e`, `--env NAME`, `--var NAME=VALUE` | As for `rt send`; `--var` applies to every request |
| `--data FILE` | CSV (header row, then one row per iteration) or JSON (an array of objects); each row's values are variables of its iteration |
| `--iterations N` | Run the requests `N` times; with `--data`, take `N` rows, starting over at the end |
| `--delay MS` | Wait `MS` milliseconds between requests |
| `--bail` | Stop at the first failed request, test or assertion |
| `--reporter NAME[=FILE]` | `text`, `json` or `junit`, repeatable. Without `FILE` the report goes to stdout (one reporter at most); missing folders of `FILE` are created |
| `--out FILE` | The file of the reporter just before it: `--reporter junit --out junit.xml` is `--reporter junit=junit.xml` |
| `--body-limit`, `--plugin`, `--no-plugins`, `--cookie-jar`, `--no-prompt`, `--no-scripts` | As for `rt send` |

### Reporters

**text** is always on: one line per request on stderr while the run goes, failed tests and
assertions, warnings and script output (`console.log`) indented below, a summary at the end.
`--reporter text=FILE` also writes it to a file.

```text
rt run: Shop API, 3 requests
✓ GET List products  200  41 ms
✗ POST Create order  201  88 ms
    ✗ order has an id: expected undefined to be a string
    > created order with total 12.50
- DELETE Cleanup  skipped

Requests: 3 (2 passed, 1 failed, 1 skipped)
Tests: 4 passed, 1 failed
Assertions: 2 passed, 0 failed
Duration: 152 ms
```

**json** prints `{"version": 1, "summary": {...}, "steps": [...]}`. `summary` counts
requests (`passed`, `failed`, `skipped`), `tests_passed`/`tests_failed`,
`assertions_passed`/`assertions_failed`, `duration_ms` and `stopped`. Each step has
`iteration` (0-based), `request_path`, `request_name`, `passed`, `skipped` and `report`, the
send report exactly as `rt send` prints it (secret values as `***`; the response body as
received, at most `--body-limit` bytes).

**junit** writes JUnit XML as GitHub, GitLab and Jenkins read it: a `<testsuite>` per request
(named by its path, plus ` [iteration n]` when the run has several), a `<testcase>` per test
and per assertion (`res.status eq 200`), a failed one with `<failure message="...">`. A
request that failed (connection, preparation, auth, a plugin or a pre-request script) is a
testcase `request` with an `<error>`; a skipped request is a skipped testcase `request`; a
request without tests and assertions is a passing testcase `request`, so every request is
listed. Script output goes to `<system-out>`. Times are in seconds.

### Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Every request got a response and every test and assertion passed (an HTTP 500 without a test is still a pass) |
| 1 | No network stack could be set up |
| 2 | Usage error: bad arguments, no collection, not a folder of it, unknown environment, unreadable data file, a folder without requests, a report that cannot be written |
| 3 | External secrets or plugins could not be set up |
| 4 | A request failed, or a test or assertion failed |
| 5 | A policy of this installation refused a request (takes precedence over 4) |

### In CI

Pass secrets as environment variables of the job and hand them to `--var`; never write them
into the collection. With `--no-prompt`, an OAuth 2.0 flow that needs a person fails instead
of waiting for a browser.

GitHub Actions, with the action in [`github-action/`](../../github-action/README.md):

```yaml
- uses: clouddnagmbh/requesttailor/github-action@v1
  with:
    collection: api
    environment: Staging
    junit: reports/junit.xml
  env:
    RT_VAR_token: ${{ secrets.API_TOKEN }}
```

GitLab CI, with the [Docker image](#docker):

```yaml
api-tests:
  image:
    name: ghcr.io/clouddnagmbh/rt:latest
    entrypoint: [""]
  script:
    - rt run api -e Staging --var "token=$API_TOKEN" --no-prompt --reporter junit=junit.xml
  artifacts:
    when: always
    reports:
      junit: junit.xml
```

Jenkins (or any shell): download `rt` for the agent's OS from the release, check it against
`SHA256SUMS`, then `rt run api --reporter junit=junit.xml` and publish `junit.xml` with the
JUnit plugin.

### Docker

`ghcr.io/clouddnagmbh/rt` is `rt` on a minimal base image (distroless, CA certificates,
no shell), running as a non-root user in `/work`, with `rt` as the entrypoint:

```sh
docker run --rm -v "$PWD:/work" ghcr.io/clouddnagmbh/rt run . --reporter junit=junit.xml
```

The container writes files as its own user (uid 65532); give that user write access to the
mounted folder for reports, or add `--user "$(id -u):$(id -g)"`. To reach a server on the
host, use `--network host` (Linux) or `host.docker.internal` as the host name (Docker
Desktop). Installed plugins are not in the image; pass them with `--plugin`. Build it
yourself from a checkout with `docker build -f docker/Dockerfile -t rt .`
([`docker/Dockerfile`](../../docker/Dockerfile)).

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
| `-f`, `--format FORMAT` | `postman` (v2.0, v2.1), `insomnia` (v4, v5), `openapi` (3.0 to 3.2, Swagger 2.0), `curl`, `har` (1.2) or `http` (VS Code REST Client, JetBrains); default: detected per file |
| `--json` | Print `{ collectionName, out, files, report }` instead of text |

Several files form one collection: a Postman collection with its environment and globals
files; `.http` files (one folder each) with `http-client.env.json` and
`http-client.private.env.json`; several cURL files (one folder each); a multi-file OpenAPI
spec (the main file and the files its `$ref`s name). Swagger 2.0 is converted to OpenAPI 3.

### OpenAPI with choices: `rt import openapi`

```sh
rt import openapi petstore.yaml --out ./petstore --names operation-id --keep-spec
rt import openapi api/openapi.yaml api/schemas/*.yaml --out ./api --group path
rt import openapi spec.zip --out ./api
rt import openapi https://api.example.com/v1/openapi.yaml --out ./api --json
```

The same choices as the app's import wizard:

| Option | Meaning |
| --- | --- |
| `FILE...` | The spec: one file (OpenAPI 3.0 to 3.2 or Swagger 2.0, JSON or YAML), several files of a multi-file spec (named by their paths relative to the folder they share, so relative `$ref`s resolve), or a `.zip` of one |
| `URL` | Fetch the spec and, recursively, the files its relative `$ref`s name (same origin only, at most 64 files); a reference that cannot be fetched is in the report, a main file that cannot be fetched exits with 1 |
| `--names operation-id` | Name requests after the `operationId` (default `summary`: the summary, else the `operationId`, else `METHOD /path`) |
| `--group tag\|path\|flat` | Folders per first tag (default), per first path segment, or none |
| `--keep-spec` | Keep the spec in the collection (`openapi.yaml` or `openapi.json` with the original bytes; a multi-file spec bundled into one `openapi.json`) and name it in `extensions.requesttailor.contract.spec`, for the contract check |
| `-o`, `--out DIR`, `--json` | As above |

Referenced files that were not given and URL references are reported, not fetched (except
the relative ones of a URL given on the command line).

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

## Git for APIs

These commands read the collection straight from Git's object store (no checkout, the working
folder is never touched) and work on the request model, not on YAML lines. A revision is a
commit id (full or abbreviated), a branch (`main`, `origin/main`), a ref or `HEAD`, each
optionally followed by `~<n>` or `^`. `<collection>` defaults to the current folder.

### Semantic diff: `rt diff`

```sh
rt diff                      # saved files against HEAD
rt diff HEAD                 # a commit against its first parent
rt diff main..feature        # two revisions (a.. means a..HEAD, ..b means HEAD..b)
rt diff HEAD~3..HEAD --json  # the CollectionDiff as JSON
```

One block per changed request, folder, environment or collection: `added`, `removed`,
`modified` or `moved` (a rename, matched by content), with one line per changed field
(`url`, `method`, `headers.Accept`, `body`, `auth`, `scripts.before-request`, ...) and its old
and new value. Secret values never appear, neither in text nor in JSON: a changed secret shows
as `<secret>`. Changes the model does not show are listed under "Not fully covered" (see `git diff` for them; the app shows their line diff). An argument
that is an existing folder is the collection, anything else is a revision. Exit code 0, also
when there are differences; 2 for bad arguments, an unreadable collection or an unknown
revision.

### Merge driver: `rt merge-driver`

Git merges collection files field by field when the driver is set up once per repository:

```sh
git config merge.requesttailor.driver "rt merge-driver %O %A %B %P"
echo "*.yml merge=requesttailor" >> .gitattributes
```

`git config` is per clone (every contributor runs it once); `.gitattributes` is committed.
Git then calls the driver for every `*.yml` file both sides changed, in `git merge`, `git
pull`, `git rebase` and `git cherry-pick`. Two branches that change different fields of one
request (say the URL on one, a header on the other) merge cleanly. Only when both sides change
the same field differently is it a conflict: exit 1, the file keeps our value in each
conflicted field with a YAML comment above it that shows theirs, everything else merged, and
stderr names the fields. Resolve it in the app (Git panel, per field) or by hand, then `git
add` as usual. Untouched parts of a file stay byte for byte as they were. Files outside a
collection merge line by line, like Git's own text merge.

### Bisect: `rt bisect`

```sh
rt bisect --good v1.4.0 --bad HEAD --env Staging api/orders
```

Finds the first commit between `--good` (excluded) and `--bad` at which a request, test or
assertion of the collection (or of `<folder>`) fails. The candidates are the first-parent
chain; each tested commit runs its own requests with today's environments and secrets (the
working folder's and the keychain's, as for `rt run`), halving the range each time, so 50
commits take at most 7 runs. A commit whose collection cannot be read is skipped. Progress
goes to stderr (`tested <id> good|bad|skip`, with what failed), the result to stdout
(`--json`: the `BisectOutcome`). Options of `rt run` apply: `--var`, `--no-scripts`,
`--no-plugins`, `--no-prompt`, `--body-limit`.

| Code | Meaning |
| --- | --- |
| 0 | The first bad commit was found |
| 1 | No network stack |
| 2 | Usage error: unknown revision, environment or folder, not a collection or repository |
| 3 | Secrets or plugins could not be set up |
| 4 | No failing commit: `--bad` passes, or only skipped commits are left |

`rt bisect`, like `rt run`, keeps no results per commit; runs in the app's compare and bisect
do (see the request history in the app).

### API changelog: `rt changelog`

```sh
rt changelog v1.4.0..HEAD > CHANGELOG-api.md
```

Writes a Markdown changelog for a range: the changes of the OpenAPI spec, with a section
"Breaking changes" (removed operation or field, changed type, new required parameter) and the
requests each change affects, then the changes of the collection. The spec is the one
`opencollection.yml` names in `extensions.requesttailor.contract.spec` (a path relative to the
collection, or to the repository root with a leading `/`), otherwise the first of
`openapi.yaml`, `openapi.yml`, `openapi.json` next to the collection, then at the repository
root. `--out FILE` writes to a file instead of stdout. Exit code 0 when written, 2 for usage
errors, an unknown revision or a spec that cannot be read.

## MCP server: `rt mcp`

`rt mcp` offers a collection to coding agents as tools, over the Model Context Protocol on
stdin/stdout (JSON-RPC, protocol versions 2025-06-18, 2025-03-26 and 2024-11-05). The client
starts the process; diagnostics go to stderr.

```sh
rt mcp [<collection>] [--var NAME=VALUE]... [--env-default NAME] [--approval-timeout SECONDS]
       [--no-scripts] [--agent-name NAME] [--body-limit BYTES]
rt approvals [--json]
rt approve <id>
rt deny <id>
```

Claude Code:

```sh
claude mcp add requesttailor -- rt mcp /path/to/collection
```

Other clients (Cursor, VS Code, Claude Desktop, ...) take the same command in their JSON
configuration:

```json
{
  "mcpServers": {
    "requesttailor": {
      "command": "rt",
      "args": ["mcp", "/path/to/collection", "--env-default", "DEV"]
    }
  }
}
```

| Option | Meaning |
| --- | --- |
| `--var NAME=VALUE` | Variable for every send, e.g. a token (repeatable). Treated as secret: the agent only sees `***`, and it may not override it |
| `--env-default NAME` | Environment when the agent names none |
| `--approval-timeout SECONDS` | How long a writing request against production waits for approval (default 300); then it fails and nothing is sent |
| `--no-scripts` | Do not run scripts |
| `--agent-name NAME` | Name for approvals and the audit log when the client sends none |
| `--body-limit BYTES` | Response body bytes the agent gets per request (default 64 KiB) |

### Tools

| Tool | Arguments | Annotations | What it does |
| --- | --- | --- | --- |
| `list_requests` | none | read-only | Path, name, method and effect (`read` or `write`) of every request |
| `get_request` | `path` | read-only | One request as in its file (URL, headers, body, auth, scripts); credentials masked, secrets only as `{{variable}}` names |
| `list_environments` | none | read-only | Environments the agent may use, with access, production flag and variables (secret ones by name only, without a value) |
| `send_request` | `path`, `environment`?, `vars`? | destructive | Sends one request and returns the masked report: status, headers, body, tests, assertions |
| `run_folder` | `folder`?, `environment`?, `vars`? | destructive | Runs a folder (empty: the whole collection) like `rt run`; every request is checked on its own; returns a summary and one report per request |

`environment` defaults to `--env-default`. `vars` are the agent's variables for this call
(see the security model below). Refusals, denials and timeouts come back as results with
`isError: true` and say that nothing was sent. Response bodies are cut at `--body-limit`
(the report says `truncated`); `size` still counts all bytes.

The effect of a request: `GET`, `HEAD`, `OPTIONS` and `TRACE` read, everything else writes.
GraphQL reads unless the document contains a mutation; gRPC writes; WebSocket writes as soon
as it sends messages.

### Access per environment

What agents may do is part of the collection, in `opencollection.yml`, and set in the app
under "Agent access" (command palette):

```yaml
extensions:
  requesttailor:
    agents:
      access: read                 # default for every environment: none, read or write
      environments:
        DEV: { access: write }
        PRD: { access: write, production: true }
        HR: none                   # hidden from agents
```

- `none`: agents do not see the environment and cannot send in it.
- `read`: only reading requests are sent; writing ones are refused. This is the default.
- `write`: writing requests are sent, except in an environment with `production: true`:
  there every writing request waits for the user's approval.

### Approvals

A writing request against production writes an approval file to the data folder
(`agents/approvals/<id>.json`, the same folder as the desktop app's, `RT_DATA_DIR`
replaces it) and waits. The user answers in one of three places, and the first answer wins:

- the desktop app shows a banner on every screen with method, masked URL, environment and
  agent, with Approve and Deny;
- `rt approvals` lists what waits (`--json` for scripts), `rt approve <id>` and `rt deny <id>`
  answer;
- clients that support MCP elicitation ask the user directly (a yes/no form). The answer of
  the client counts as the user's: only use clients that ask a person, not the model.

Denied, timed out or cancelled (the client cancels the call or disconnects): the request is
not sent, the agent gets an error that says so, and the approval file is removed. While it
waits, `rt mcp` touches the file every 5 s; files of processes that died are cleaned up after
30 s.

### Audit log

Every tool call, plus the `initialize` of each session, appends one JSON line to
`agents/audit.jsonl` in the data folder: time, agent, tool, collection, request, environment,
method, masked URL, status, outcome (`read`, `sent`, `refused`, `denied`, `timed-out`,
`failed`), who approved (`app`, `cli`, `elicitation`) and a detail. `run_folder` writes one entry
with one detail line per request. The log stays on this computer; the desktop app shows it
under "Agent audit log". Nothing is sent anywhere.

### Security model

- **Agents never see secret values.** Reports are masked by the core. On top of that, every
  message `rt mcp` writes (results, errors, approval prompts) and every audit line passes a
  final mask that replaces every secret value the process knows (`--var` values, keychain
  values, values from secret managers; raw and percent-encoded) with `***`, also when a
  server echoes a token back in its body. Values shorter than 4 characters are only masked
  where the core knows them by name. `get_request` and `list_environments` leave out secret
  values and mask credentials (auth fields, credential headers).
- **The policy is pinned at start.** An agent that can edit files could otherwise give
  itself write access. `rt mcp` reads the policy once; later edits apply only where they are
  stricter. Loosening a rule needs a restart.
- **Agent variables cannot redirect requests.** `vars` from the agent may fill in names
  nothing else defines and override request and folder variables. They cannot override
  variables of the collection, the environment or `--var` (base URLs and credentials live
  there), nor any secret name.
- **Secrets come from where the app keeps them**: the OS keychain (shared with the desktop
  app and with `rt send` and `rt run`, see "Secret variables") and the secret managers of `extensions.requesttailor.secrets`. OAuth 2.0 logins that
  need a browser or a code are not started; use a token the app already fetched.
- **Scripts** run only in collections the user trusted in the desktop app, and not with
  `--no-scripts`. Requests a trusted script sends itself (`rt.sendRequest`) are not checked
  against the policy; do not trust collections whose scripts you do not know.
- Every call reads the files anew, so the agent works with the files as they are now.

## MCP: testing servers and the demo server

MCP (Model Context Protocol) is how AI assistants call tools on servers. `rt mcp` (above) is
one direction: your collection is the server, an agent the client. The MCP view of the app
is the other: RequestTailor is the client and you test MCP servers like APIs.

1. **Connect** to a server by the command that starts it (stdio) or by URL (Streamable HTTP).
   Headers, arguments and environment take `{{variables}}` from the active environment.
2. **Browse** what it offers: tools (with their input schema), resources and prompts, all
   pages of each list.
3. **Call** a tool with a form built from its schema (or raw JSON) and see the result:
   structured output, text, images, `isError`, and the progress and log notifications the
   server sent on the way.
4. **Save** the call as a test: a `*.mcp.yaml` file next to your collection with checks on
   `res.status` (200, or 500 for a tool error), `res.body` (structured output, else the first
   text) and `res.responseTime`. Secrets stay `{{references}}`.
5. **Run all** saved tests in the app, and in CI with `rt mcp-test`.

### Demo server: `rt mcp-demo`

A small MCP server with fake data to try all of this with: offline, deterministic, no account.

| Kind | Name | What it shows |
| --- | --- | --- |
| Tool | `echo` | The simplest call |
| Tool | `add` | Structured output with an `outputSchema` |
| Tool | `fake_weather` | Form with a required and an enum argument; the same city always gets the same weather |
| Tool | `todo_add`, `todo_list`, `todo_done` | State per session (kept until you disconnect) |
| Tool | `uuid` | Deterministic per session, so saved tests stay stable |
| Tool | `image` | Image content (a PNG) |
| Tool | `fail` | A tool error (`isError: true`) |
| Tool | `countdown` | Progress and log notifications before the result |
| Resource | `demo://readme`, `demo://cities.json`, `demo://todos` | Text, JSON, live state |
| Prompt | `summarize`, `weather_report` | Arguments, an embedded resource |

Lists are paginated (tools four per page) so clients have to follow `nextCursor`.

Where it runs:

- **In the app (desktop and web):** "Try the demo server" connects to the built-in address
  `rt-demo://mcp`. The server runs inside RequestTailor's core, so nothing is started and
  nothing goes over the network. Tests saved against it run anywhere, including
  `rt mcp-test` in CI.
- **Desktop, on a real port:** "Demo server on a local port" starts the same server on
  `http://127.0.0.1:<free port>/mcp` for other MCP clients; it stops when you stop it or quit
  the app.
- **Command line:** `rt mcp-demo` speaks MCP on stdin/stdout; `rt mcp-demo --http` serves
  Streamable HTTP on `http://127.0.0.1:47850/mcp` (`--port 0` picks a free port) until
  Ctrl-C. Only loopback; requests with another `Host` or `Origin` are refused.

```json
{ "mcpServers": { "requesttailor-demo": { "command": "rt", "args": ["mcp-demo"] } } }
```

"Connect to RequestTailor itself" in the app starts `rt mcp` on the open collection (desktop,
when `rt` is installed) so you see your requests as an agent sees them. The web app cannot
start commands and says so.

### Testing MCP servers: `rt mcp-test`

`rt mcp-test [<path>...] [-e NAME] [--var NAME=VALUE]...` runs the saved MCP tests of a
collection (`*.mcp.yaml` files next to the requests, made in the app) against their servers
(stdio, Streamable HTTP or the built-in demo `rt-demo://mcp`) and prints
`{"tests", "passed", "failed"}` as JSON with secrets shown as `***`. Exit code 0 when all
passed, 4 when a test failed, 1 when a server could not be reached or started, 2 on a usage
error or when there are no tests, 3 when external secrets could not be read.

## Send guard (for distributions)

A distribution that builds its own `rt` from this crate can add a check in front of every
request: `rt_cli::run_distribution(args, stdout, stderr, &Distribution { extensions, guard })`
with a `rt_core::guard::SendGuard`. The builds of this repository pass none.

- `rt send`, `rt run`, and the tools `send_request` and `run_folder` of `rt mcp` ask the
  guard once a request is final (after scripts, `prepare` and plugin hooks) and before it
  goes out; requests a script sends with `rt.sendRequest` are asked too. The guard sees who
  sends (`cli` or `agent`), method, URL with secret values masked, whether the request
  writes, and whether the collection marks the environment as production; never a secret
  value, a header value or a body.
- A refused request is not sent. `rt send` and `rt run` exit with 5 and the report's error
  is `{"kind": "blocked", "message": {"rule", "reason"}}`. `rt mcp` asks the guard before an
  approval, answers with the reason (`isError: true`) and writes the rule into the audit
  line (`"outcome": "refused", "rule": …`).
- The desktop app (`rt_desktop::builder().manage(GuardHandle::new(guard))`) and the web app
  (`createWebBackend({ sendGuard })`) ask the same way, with caller `app`.

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
   offer to pair again). The session relay `GET /v1/stream` (wave 8) takes the token as
   `?token=` because browsers cannot set headers on a WebSocket; the `Host` and `Origin`
   checks apply to the upgrade as to every request (browsers always send `Origin` on a
   WebSocket), and a wrong or missing token gets `401` before the upgrade. Tokens are compared in constant time and live in memory only:
   nothing is written to disk, and a restart revokes all of them.
6. **Secrets only when asked for.** `POST /v1/secrets` answers `403` unless the companion
   was started with `--allow-secrets` (the start banner says so), and like send it needs an
   allowed `Origin` and that origin's token. It runs only the five managers' CLIs, with
   fixed arguments built from the references (no shell; references that start with `-` or
   contain whitespace are refused), a 60 s timeout and your existing logins; it keeps no
   values (no cache, nothing on disk). Answers carry `Cache-Control: no-store`. A page on an
   allowed origin with a token can read any secret your CLI logins can, so enable the flag
   only for origins you trust as much as your terminal.
7. **Nothing else.** Five endpoints, no file access, no configuration over HTTP. The
   companion refuses to send requests to itself, and to open sessions to itself. A session
   WebSocket carries exactly one session: the first message must be `open` (within 10 s),
   and the relay ends when either side does. Requests are capped at 64 MiB (`413`), the
   pairing request at 4 KiB, the secrets request at 64 KiB; the request head must arrive
   within 10 s.
8. **Logging.** One line per request on stderr with method and host only (`rt agent: GET
   api.internal:8443 -> 200 (35 ms to first byte)`), plus pairings and rejections. Paths,
   queries, headers and bodies are never logged. For secrets, only the provider, the number
   of values and the origin (`rt agent: read 2 secret(s) from 1Password for
   http://127.0.0.1:5173`); never references or values. Sessions log their kind, host and
   origin when they open and close (`rt agent: WebSocket api.internal:443 for
   http://127.0.0.1:5173: open`), never URLs, the token, headers or messages.

What it does not protect against: other programs running as you on the same machine can
talk to `127.0.0.1` too, but they cannot pair without the code printed in your terminal. A
compromised page on an allowed origin (XSS) can use that origin's token like the web app
itself; allow only origins you trust.

### Protocol (version 1)

Types: `rt_core::transport::companion`.

| Endpoint | Request | Answer |
| --- | --- | --- |
| `GET /v1/status` | optional `Authorization: Bearer <token>` | `200 {"protocol": 1, "version": "...", "paired": bool, "features": ["stream"]}`; `paired` is true when the token belongs to the `Origin`; `features` lists optional endpoints |
| `POST /v1/pair` | `{"code": "7KQ2-M9XD"}` (dash, spaces and case do not matter) | `200 {"token": "..."}`, or `403 {"error": "wrong code" \| "code expired" \| ...}` |
| `POST /v1/secrets` | `Authorization: Bearer <token>`, JSON `SecretsRequest` (`{"provider": "one-password", "variables": [{"name": "...", "reference": "...", "enabled": true}]}`) | `200 {"values": [["name", "value"], ...]}` for the enabled entries; `422 {"error": "..."}` when a secret cannot be read (CLI missing, not signed in, timeout); `403 {"error": "..."}` without `--allow-secrets`; `401` without a valid token |
| `POST /v1/send` | `Authorization: Bearer <token>`, JSON `WireRequest` (method, url, headers, base64 body, content type, timeout, redirect settings) | `200`, header `x-rt-head` with the JSON `WireHead` (status, version, headers, final URL, redirects, timing; non-ASCII escaped), body = the response body, streamed; `502` with a JSON transport error (`{"kind": "timeout"}`, `{"kind": "connect", "message": "..."}`, ...); `401` without a valid token |
| `GET /v1/stream?token=<token>` | WebSocket upgrade (wave 8); then JSON text messages `StreamClientMessage`: first `{"op": "open", "open": {"kind": "websocket" \| "sse" \| "grpc", ...}}`, then `{"op": "send", "frame": {"type": "text", "data": "..."}}`, `{"op": "finish"}`, `{"op": "close"}` | `101`; then `StreamServerMessage`s: `{"op": "event", "event": {...}}` for every event of the native session (`opened` first, `closed` or `error` last), or `{"op": "failed", "error": {...}}` when opening or sending failed; the WebSocket then closes. `401` without a valid token, `400` without a WebSocket upgrade |

The timeout and redirect settings of the `WireRequest` apply exactly as in `rt send`; the
timeout covers the whole exchange including the body. If it fires while the body is
streaming, the connection is cut and the web app reports a timeout.
