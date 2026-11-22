// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `RescueApi` of the mock backend (wave 12, strand G): a history with a GitHub token in a
// request and a private key next to it, found on the mock's commits and branches. The rewrite
// checks the plan as the shells do (unsaved changes, variable name, private keys), puts the
// value into the mock's secret store and reports `main` as pushed (so the UI shows the force
// push warning) and a tag on a rewritten commit. A scan after the rescue finds only the key.
// `host` (./mock-wave12.ts) is read only inside the methods.

import { BackendError, type RescueApi } from "./backend";
import type { MockWave12Host } from "./mock-wave12";
import type { GitCommit, HistoryFinding, HistoryScan, RescueResult } from "./types";

/** The value the mock "finds" (assembled at run time, so secret scanners stay quiet). It
 *  only ever goes into the mock's secret store, as in the real shells. */
export const MOCK_RESCUE_VALUE = ["ghp_", "M0ck", "Rescue", "Value", "0123456789abcdefABCDEF"].join("");

const BACKUP_PREFIX = "refs/requesttailor/backup/";

const fakeOid = (n: number) => n.toString(16).padStart(8, "0").repeat(5);

export function mockRescue(host: MockWave12Host): RescueApi {
  let rescued = false;

  async function history(): Promise<{ commits: GitCommit[]; branches: string[] }> {
    const git = host.git();
    let commits = await git.log(100).catch(() => [] as GitCommit[]);
    if (commits.length === 0) {
      commits = [{ oid: fakeOid(1), summary: "Add the user requests", author: "Ada", time: 1_790_000_000 }];
    }
    const branches = (await git.branches().catch(() => []))
      .filter((b) => !b.remote)
      .map((b) => b.name);
    return { commits: [...commits].reverse(), branches: branches.length > 0 ? branches : ["main"] };
  }

  function findings(commits: GitCommit[], branches: string[]): HistoryFinding[] {
    const first = commits[0];
    const token: HistoryFinding = {
      id: `${first.oid}:users/list-users.yml:9`,
      finding: {
        path: "users/list-users.yml",
        line: 9,
        kind: "GitHub token",
        snippet: "value: Bearer ***",
        hint:
          "Make it a secret variable: in Environments add `githubToken`, mark it as secret (the value is kept " +
          "in the OS keychain, or comes from a secret manager under External secrets) and write `{{githubToken}}` " +
          "here instead. Not a credential? Add `# rt-scan:allow` to the line.",
      },
      first,
      commits: commits.map((c) => c.oid),
      branches,
      in_head: true,
    };
    const key: HistoryFinding = {
      id: `${first.oid}:certs/client.pem:1`,
      finding: {
        path: "certs/client.pem",
        line: 1,
        kind: "private key",
        snippet: "***",
        hint:
          "Do not commit private keys: keep the key file out of Git (add it to .gitignore) and reference it " +
          "from the client certificate settings; its passphrase belongs in a secret variable.",
      },
      first,
      commits: [first.oid],
      branches,
      in_head: false,
    };
    return rescued ? [key] : [token, key];
  }

  return {
    async scanHistory(onProgress) {
      const { commits, branches } = await history();
      const total = commits.length;
      onProgress?.({ scanned: 0, total });
      for (let i = 1; i <= total; i++) {
        await new Promise((r) => setTimeout(r, 15));
        onProgress?.({ scanned: i, total });
      }
      const scan: HistoryScan = { commits: total, findings: findings(commits, branches) };
      return scan;
    },

    async rewrite(plan) {
      if ((await host.backend.dirty()).length > 0) {
        throw new BackendError("git", "Save or discard your changes before you rewrite the history.");
      }
      if (plan.findings.length === 0) throw new BackendError("git", "choose a finding to rescue");
      if (!/^[A-Za-z_][A-Za-z0-9_.-]*$/.test(plan.variable)) {
        throw new BackendError("git", `"${plan.variable}" is not a variable name: use letters, digits, \`_\`, \`-\` and \`.\``);
      }
      if (!plan.environment.trim()) throw new BackendError("git", "choose the environment that keeps the secret");
      const { commits, branches } = await history();
      const known = findings(commits, branches);
      for (const id of plan.findings) {
        const f = known.find((k) => k.id === id);
        if (!f) throw new BackendError("git", "a finding is no longer where the scan saw it: scan the history again");
        if (f.finding.kind.includes("private key")) {
          throw new BackendError(
            "unsupported",
            "A private key cannot become a variable: remove the key file from the history with a tool such as git filter-repo, and replace the key.",
          );
        }
      }
      await host.backend.secrets.set(plan.environment, plan.variable, MOCK_RESCUE_VALUE);
      rescued = true;
      const unix = Math.floor(Date.now() / 1000);
      const result: RescueResult = {
        rewritten: commits.length,
        branches: branches.map((name, i) => ({
          name,
          old: fakeOid(100 + i),
          new: fakeOid(200 + i),
          pushed: name === "main",
          remote: name === "main" ? "origin" : null,
        })),
        backup: `${BACKUP_PREFIX}${unix}/`,
        tags: ["v1.0"],
      };
      return result;
    },
  };
}
