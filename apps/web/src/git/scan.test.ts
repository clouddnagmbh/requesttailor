// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The web shell's secret check before a commit (wave 5, strand E), on an in-memory folder.
// The scanner itself is the core's (`scanSecrets`, tested in Rust and through the real core in
// the shells end-to-end tests); here a stand-in finds lines with the fake token, so the test
// checks what the shell does: which files it reads, that `commit` refuses with
// `secrets-found` and commits nothing, and that `scan` lists the findings.

import { BackendError, type SecretFinding } from "@requesttailor/ui";
import { describe, expect, it } from "vitest";

import type { WebHost } from "../web-backend";
import type { CoreModule } from "../wasm";
import { fill, memoryFolder } from "./fake-handle.test-util";
import { webGit } from "./index";
import { MAX_SCAN_BYTES, readForScan, textOf } from "./scan";

// Assembled at run time, so secret scanners of the repository stay quiet.
const TOKEN = ["9c1e2f3a4b5c", "6d7e8f90a1b2"].join("");

const scanned: string[][] = [];
const fakeCore = {
  scanSecrets(files: [string, string][]): SecretFinding[] {
    scanned.push(files.map(([p]) => p));
    return files.flatMap(([path, text]) =>
      text.split("\n").flatMap((line, i) =>
        line.includes(TOKEN) && !line.includes("rt-scan:allow")
          ? [{ path, line: i + 1, kind: "bearer token", snippet: line.replace(TOKEN, "***").trim(), hint: "Make it a secret variable." }]
          : [],
      ),
    );
  },
  secretRefusal(findings: SecretFinding[]): string {
    return `Commit refused: ${findings.length} credential(s) (${findings[0].path}:${findings[0].line}). ${findings[0].hint}`;
  },
} as unknown as CoreModule;

function hostFor(handle: FileSystemDirectoryHandle): WebHost {
  return {
    deps: {
      loadCore: async () => fakeCore,
      openSecrets: async () => {
        throw new Error("not used");
      },
      companion: () => {
        throw new Error("not used");
      },
    },
    session: () => ({ id: "c", handle, ws: { dirty: () => [] } }) as unknown as ReturnType<WebHost["session"]>,
    openHandle: async () => {
      throw new Error("not used");
    },
    companionCredentials: async () => null,
  };
}

const request = (auth: string) =>
  `info:\n  name: Me\n  type: http\n\nhttp:\n  method: GET\n  url: https://api.example.com/me\n  headers:\n    - name: Authorization\n      value: Bearer ${auth}\n`;

describe("textOf", () => {
  it("skips what the scanner skips", () => {
    expect(textOf(new TextEncoder().encode("token: x"))).toBe("token: x");
    expect(textOf(new Uint8Array([0x61, 0, 0x62]))).toBeNull();
    expect(textOf(new Uint8Array([0xff, 0xfe, 0x41]))).toBeNull();
    expect(textOf(new Uint8Array(MAX_SCAN_BYTES + 1).fill(0x61))).toBeNull();
  });
});

describe("readForScan", () => {
  it("reads existing text files of the commit and leaves out deleted ones", async () => {
    const root = memoryFolder();
    await fill(root, { "a.yml": "x: 1\n", "dir/b.yml": "y: 2\n" });
    expect(await readForScan(root, ["a.yml", "dir/b.yml", "gone.yml"])).toEqual([
      ["a.yml", "x: 1\n"],
      ["dir/b.yml", "y: 2\n"],
    ]);
  });
});

describe("webGit with the secret scanner", () => {
  it("refuses a commit with a plaintext token and lists it in scan", async () => {
    const root = memoryFolder();
    await fill(root, { "opencollection.yml": 'opencollection: "1.0.0"\n', "users/me.yml": request(TOKEN) });
    const git = webGit(hostFor(root));
    await git.init();
    await git.setIdentity({ name: "Ada", email: "ada@example.com" });
    const paths = ["opencollection.yml", "users/me.yml"];

    const findings = await git.scan(paths);
    expect(findings).toEqual([
      { path: "users/me.yml", line: 10, kind: "bearer token", snippet: "value: Bearer ***", hint: "Make it a secret variable." },
    ]);

    scanned.length = 0;
    const err = await git.commit("Add Me", paths).then(
      () => null,
      (e: unknown) => e,
    );
    expect(err).toBeInstanceOf(BackendError);
    expect((err as BackendError).kind).toBe("secrets-found");
    expect((err as BackendError).message).toContain("users/me.yml:10");
    expect((err as BackendError).message).not.toContain(TOKEN);
    // The commit scanned exactly its files, and nothing was committed.
    expect(scanned).toEqual([paths]);
    expect(await git.log(10)).toEqual([]);

    // With a variable, the same commit goes through.
    await fill(root, { "users/me.yml": request("{{token}}") });
    expect(await git.scan(paths)).toEqual([]);
    await git.commit("Add Me", paths);
    expect((await git.log(10)).map((c) => c.summary)).toEqual(["Add Me"]);
  });

  it("commits a line marked rt-scan:allow", async () => {
    const root = memoryFolder();
    await fill(root, { "example.yml": `${request(TOKEN).trimEnd()} # rt-scan:allow\n` });
    const git = webGit(hostFor(root));
    await git.init();
    await git.setIdentity({ name: "Ada", email: "ada@example.com" });
    await git.commit("Example", ["example.yml"]);
    expect(await git.log(10)).toHaveLength(1);
  });
});
