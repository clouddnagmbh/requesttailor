# rt

The RequestTailor command line. Wave 2 has one command, `rt send`.

```sh
rt send <request-file> [--env NAME] [--var NAME=VALUE]... [--body-limit BYTES] [--pretty]
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
