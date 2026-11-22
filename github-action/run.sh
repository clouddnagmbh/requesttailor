#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 The RequestTailor Authors
#
# Runs `rt run` with the action's inputs (INPUT_* environment variables, see action.yml), then
# sets the outputs and the job summary from the JSON report (summary.mjs) and exits with rt's
# exit code.
#
# Secrets: every environment variable RT_VAR_<NAME> becomes `--var <NAME>=<value>`. Values
# are never printed; the arguments are not echoed.
set -euo pipefail

fail() {
  echo "::error::$1" >&2
  exit 2
}

rt="${RT_BIN:-}"
[ -n "$rt" ] || fail "No rt binary (install step skipped and rt-path empty)."
[ -x "$rt" ] || fail "rt-path $rt is not an executable file."

trim() {
  local s="$1"
  s="${s#"${s%%[![:space:]]*}"}"
  s="${s%"${s##*[![:space:]]}"}"
  printf '%s' "$s"
}

is_true() {
  case "$(trim "${1:-}")" in true | True | TRUE | yes | 1) return 0 ;; *) return 1 ;; esac
}

args=(run "$(trim "${INPUT_COLLECTION:-.}")" --no-prompt)
env_name=$(trim "${INPUT_ENVIRONMENT:-}")
[ -z "$env_name" ] || args+=(--env "$env_name")
data=$(trim "${INPUT_DATA:-}")
[ -z "$data" ] || args+=(--data "$data")
iterations=$(trim "${INPUT_ITERATIONS:-}")
[ -z "$iterations" ] || args+=(--iterations "$iterations")
if is_true "${INPUT_NO_SCRIPTS:-}"; then args+=(--no-scripts); fi
if is_true "${INPUT_BAIL:-}"; then args+=(--bail); fi

# vars: one NAME=VALUE per line.
line_no=0
while IFS= read -r line || [ -n "$line" ]; do
  line_no=$((line_no + 1))
  line=$(trim "$line")
  case "$line" in "" | "#"*) continue ;; esac
  case "$line" in
    [A-Za-z_]*=*) args+=(--var "$line") ;;
    *) fail "vars, line $line_no: expected NAME=VALUE." ;;
  esac
done <<< "${INPUT_VARS:-}"

# Secrets from the step's environment: RT_VAR_<NAME>.
while IFS= read -r name; do
  case "$name" in
    RT_VAR_?*) args+=(--var "${name#RT_VAR_}=${!name}") ;;
  esac
done < <(compgen -e)

junit=$(trim "${INPUT_JUNIT:-}")
[ -z "$junit" ] || args+=(--reporter "junit=$junit")
report=$(trim "${INPUT_JSON:-}")
if [ -z "$report" ]; then
  report="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/rt-run-${GITHUB_RUN_ID:-local}-$$.json"
fi
args+=(--reporter "json=$report")

# More arguments, one per line, as they are.
while IFS= read -r line || [ -n "$line" ]; do
  line=$(trim "$line")
  [ -z "$line" ] || args+=("$line")
done <<< "${INPUT_ARGS:-}"

# Nothing rt prints (response data, script output) may act as a workflow command.
token="rt-$(od -An -N8 -tx1 /dev/urandom | tr -d ' \n')"
echo "::stop-commands::$token"
set +e
"$rt" "${args[@]}"
code=$?
set -e
echo "::$token::"

out="${GITHUB_OUTPUT:-/dev/null}"
{
  echo "exit-code=$code"
  echo "rt-path=$rt"
  if [ -n "$junit" ] && [ -f "$junit" ]; then echo "junit=$junit"; else echo "junit="; fi
} >> "$out"

if [ -f "$report" ]; then
  echo "report=$report" >> "$out"
  if command -v node > /dev/null; then
    summary_file=/dev/null
    if is_true "${INPUT_SUMMARY:-true}"; then summary_file="${GITHUB_STEP_SUMMARY:-/dev/null}"; fi
    node "$(dirname "$0")/summary.mjs" "$report" "$code" "$out" "$summary_file" ||
      echo "::warning::Could not read the JSON report for the outputs and the job summary."
  else
    echo "::warning::node is not on the PATH: no outputs from the report and no job summary."
  fi
else
  echo "report=" >> "$out"
fi

case "$code" in
  0) ;;
  4) echo "::error::rt run: a request, test or assertion failed (see the log above and the job summary)." ;;
  *) echo "::error::rt run stopped with exit code $code (2: usage error, see the message above)." ;;
esac
exit "$code"
