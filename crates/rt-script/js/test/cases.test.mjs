// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Runs the shim and module case files (crates/rt-script/tests/cases/{shims,modules}) against
// the fake engine. After strand A the same files run natively and in the WASM core
// (`cargo test -p rt-script --test cases`).

import assert from "node:assert/strict";
import { join } from "node:path";
import { test } from "node:test";

import { CASES_DIR, caseFiles, compareOutcome, runPhase } from "./fake-rt.mjs";

for (const dir of ["shims", "modules"]) {
  for (const { file, case: c } of caseFiles(join(CASES_DIR, dir))) {
    test(`${dir}/${file.split(`/${dir}/`)[1]}: ${c.name}`, async () => {
      assert.ok(c.name && c.phase && c.scripts && c.request, "name, phase, scripts and request are required");
      const outcome = await runPhase(c);
      const diffs = compareOutcome(outcome, c.expect);
      assert.deepEqual(diffs, [], diffs.join("\n"));
    });
  }
}
