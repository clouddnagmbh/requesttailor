<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# RequestTailor run (GitHub Action)

Runs an API collection in a GitHub Actions job with [`rt run`](../crates/rt-cli/README.md#collection-runs-rt-run):
OpenCollection YAML as RequestTailor and Bruno write it, or a Postman, Insomnia or OpenAPI
export after `rt import`. The same core, scripts and assertions as the desktop and web apps:
what is green in the app is green in the pipeline.

The action downloads `rt` for the runner from the project's GitHub release, checks it against
the release's `SHA256SUMS`, runs the collection, writes a JUnit report and a JSON report,
sets outputs and adds a table of the requests to the job summary. The step fails when a
request, test or assertion fails.

Runners: Linux (x86_64, aarch64) and macOS. On Windows runners use a Linux job or the Docker
image `ghcr.io/clouddnagmbh/rt`.

## Usage

```yaml
jobs:
  api-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: clouddnagmbh/requesttailor/github-action@v1
        with:
          collection: api
          environment: Staging
        env:
          RT_VAR_apiToken: ${{ secrets.STAGING_API_TOKEN }}
```

### Secrets

Pass secret values as environment variables of the step named `RT_VAR_<NAME>`: each becomes
the runtime variable `<NAME>` (`RT_VAR_apiToken` sets `{{apiToken}}`), with the highest
precedence, for every request. The action never prints them, and `rt` shows variables marked
secret in the collection as `***` in its reports. Do not put secrets into `vars`: inputs are
shown in the log when the step starts (GitHub masks known secrets there, but values derived
from them are not masked). Never write secret values into the collection files; `rt scan`
finds them before they are committed.

OAuth 2.0 flows that need a person (authorization code, device code) cannot run in CI; the
action runs `rt` with `--no-prompt`, so they fail with a message instead of waiting. Use
client credentials or pass a token as a variable.

### With the JUnit report in the pull request

```yaml
      - uses: clouddnagmbh/requesttailor/github-action@v1
        id: api
        with:
          collection: api
          environment: Staging
          junit: reports/api-junit.xml
        env:
          RT_VAR_apiToken: ${{ secrets.STAGING_API_TOKEN }}

      # Optional: keep the reports, also when the run failed.
      - uses: actions/upload-artifact@v4
        if: always()
        with:
          name: api-reports
          path: |
            reports/api-junit.xml
            ${{ steps.api.outputs.report }}

      # Optional: show the tests as a check run (any JUnit reporter action works).
      - uses: mikepenz/action-junit-report@v5
        if: always()
        with:
          report_paths: reports/api-junit.xml
```

### Data-driven, several folders

```yaml
    strategy:
      matrix:
        folder: [api/users, api/orders]
    steps:
      - uses: actions/checkout@v4
      - uses: clouddnagmbh/requesttailor/github-action@v1
        with:
          collection: ${{ matrix.folder }}
          environment: Staging
          data: testdata/users.csv
          vars: |
            region=eu-central-1
          args: |
            --delay
            200
```

### A Postman collection

Import it once and commit the result; then the files in the repository are what runs, and
changes show up as diffs:

```sh
rt import postman/shop.postman_collection.json postman/staging.postman_environment.json --out api
git add api && git commit -s -m "API tests as OpenCollection files"
```

Scripts keep working: `pm.*` and `bru.*` calls run on RequestTailor's script API; the import
report lists what is not covered (see `crates/rt-script/SHIM-COVERAGE.md`).

## Inputs

| Input | Default | Meaning |
| --- | --- | --- |
| `collection` | `.` | The collection root or a folder inside it (a folder runs only its requests) |
| `environment` | | Name of the environment |
| `vars` | | Runtime variables, one `NAME=VALUE` per line; `#` lines are skipped. Not for secrets (see above) |
| `data` | | CSV or JSON data file; one iteration per row |
| `iterations` | | Number of iterations; with `data`, that many rows, starting over at the end |
| `junit` | `rt-junit.xml` | Path of the JUnit report; empty for none |
| `json` | temp file | Path of the JSON report (`{"version": 1, "summary", "steps"}`) |
| `no-scripts` | `false` | `true`: do not run the collection's scripts (assertions still run) |
| `bail` | `false` | `true`: stop at the first failure |
| `args` | | More `rt run` arguments, one per line (`--delay`, `--plugin`, `--body-limit`, ...) |
| `summary` | `true` | Write the job summary |
| `version` | `latest` | The `rt` release (`0.1.0` or `v0.1.0`). Pin it for reproducible runs |
| `rt-path` | | Use this `rt` binary instead of downloading one |

## Outputs

| Output | Meaning |
| --- | --- |
| `passed`, `failed`, `skipped` | Requests |
| `tests-passed`, `tests-failed` | `test(...)` results of the scripts |
| `assertions-passed`, `assertions-failed` | Declarative assertions |
| `report` | Path of the JSON report |
| `junit` | Path of the JUnit report (empty if none was written) |
| `exit-code` | `0` passed, `4` a request, test or assertion failed, `2` usage error (see the log) |
| `rt-path` | The `rt` binary that ran |

To go on after a failure (for example to post a comment with the numbers), give the step
`continue-on-error: true` and read the outputs.

## Versions

`@v1` follows the latest compatible release of the action; pin a release tag (`@v0.1.0`) or
a commit for more control. The action and `rt` are versioned with the repository; `version`
picks the `rt` release independently.

## How it checks what it runs

- `rt` comes from `https://github.com/clouddnagmbh/requesttailor/releases/download/v<version>/`
  (`rt_<version>_linux_x86_64.tar.gz`, `rt_<version>_linux_aarch64.tar.gz`,
  `rt_<version>_macos_universal.zip`); its SHA-256 must match the line in the release's
  `SHA256SUMS`, or the step stops before running anything. `SHA256SUMS` itself is signed
  (`SHA256SUMS.asc`, see RELEASING.md); to check that signature too, install `rt` yourself
  and pass `rt-path`.
- Output of `rt` (response data, script output) cannot issue workflow commands: the action
  stops command processing while `rt` runs.
- The job summary escapes everything that comes from responses and scripts.

## Development

`run.sh` and `install.sh` hold the logic, `summary.mjs` the outputs and the job summary; the
tests are in `scripts/github-action.test.mjs` (`npm run test:scripts`). CI runs the action
from this folder with a locally built `rt` (`rt-path`) against `rt-testserver`, on
`test/collection` and on the imported Postman collection of the wave 7 gate.
