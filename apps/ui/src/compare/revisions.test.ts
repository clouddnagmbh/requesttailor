// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { GitRef } from "../commands/types";
import { filterOptions, optionsFromBranches, optionsFromRefs, quickTargets, revisionLabel } from "./revisions";

const ref = (name: string, kind: GitRef["kind"], over: Partial<GitRef> = {}): GitRef => ({
  full: kind === "local" ? `refs/heads/${name}` : kind === "remote" ? `refs/remotes/${name}` : `refs/tags/${name}`,
  name,
  kind,
  target: "0".repeat(40),
  current: false,
  ahead: 0,
  behind: 0,
  ...over,
});

const commits = [{ oid: "abcdef0123456789abcdef0123456789abcdef01", summary: "Fix pets", author: "A", time: 0 }];

describe("revision picker", () => {
  const refs = [
    ref("feature/pets", "local", { current: true, upstream: "origin/feature/pets", ahead: 2, behind: 1 }),
    ref("main", "local", { upstream: "origin/main" }),
    ref("origin/main", "remote"),
    ref("origin/feature/pets", "remote"),
    ref("origin/HEAD", "remote"),
    ref("v1.2", "tag", { message: "Release 1.2" }),
    ref("v1.10", "tag"),
  ];
  const options = optionsFromRefs(refs, commits);

  it("lists branches, remotes, tags and commits on lanes", () => {
    expect(options.map((o) => [o.kind, o.label])).toEqual([
      ["branch", "feature/pets"],
      ["branch", "main"],
      ["remote", "origin/feature/pets"],
      ["remote", "origin/main"],
      ["tag", "v1.10"],
      ["tag", "v1.2"],
      ["commit", "abcdef0"],
    ]);
    const lane = (label: string) => options.find((o) => o.label === label)!.lane;
    // The current branch on lane 1, main on lane 2; a remote shares its branch's lane.
    expect([lane("feature/pets"), lane("main"), lane("origin/main"), lane("origin/feature/pets")]).toEqual([1, 2, 2, 1]);
    expect(lane("v1.2")).toBe(0);
    expect(lane("abcdef0")).toBe(1);
    expect(options[0]!.detail).toBe("2 ahead · 1 behind origin/feature/pets");
    expect(options[1]!.detail).toBe("in sync with origin/main");
    expect(options.find((o) => o.label === "v1.2")!.detail).toBe("Release 1.2");
  });

  it("offers compare with main and with the remote", () => {
    expect(quickTargets(options, "origin/feature/pets")).toEqual({ current: "feature/pets", main: "main", remote: "origin/feature/pets" });
    // On main: no "with main", the remote from the name.
    const onMain = optionsFromRefs(refs.map((r) => ({ ...r, current: r.name === "main" })), []);
    expect(quickTargets(onMain)).toEqual({ current: "main", remote: "origin/main" });
    // Without branches (detached, no repository data): HEAD only.
    expect(quickTargets([])).toEqual({ current: "HEAD" });
  });

  it("falls back to the branch list", () => {
    const o = optionsFromBranches(
      [
        { name: "main", current: false, remote: false },
        { name: "dev", current: true, remote: false },
        { name: "origin/main", current: false, remote: true },
      ],
      commits,
    );
    expect(o.map((x) => x.label)).toEqual(["dev", "main", "origin/main", "abcdef0"]);
    expect(quickTargets(o)).toEqual({ current: "dev", main: "main" });
  });

  it("filters by every word and labels revisions", () => {
    // The detail counts: the branch names its upstream.
    expect(filterOptions(options, "pets orig").map((o) => o.label)).toEqual(["feature/pets", "origin/feature/pets"]);
    expect(filterOptions(options, "v1.1").map((o) => o.label)).toEqual(["v1.10"]);
    expect(filterOptions(options, "fix").map((o) => o.kind)).toEqual(["commit"]);
    expect(filterOptions(options, "  ")).toHaveLength(options.length);
    expect(revisionLabel(commits[0]!.oid)).toBe("abcdef0");
    expect(revisionLabel("HEAD~1")).toBe("HEAD~1");
  });
});
