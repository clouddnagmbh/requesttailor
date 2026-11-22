// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { CollectionDiff, ReviewComment, ReviewRepo } from "../commands/types";
import {
  changedRequestPaths,
  deviceLoginOffered,
  fieldForLine,
  groupComments,
  relativeTime,
  withFields,
} from "./model";

const diff: CollectionDiff = {
  changes: [
    {
      target: { kind: "request", path: "orders/list.yml" },
      change: { kind: "modified" },
      file: "orders/list.yml",
      name: "List",
      fields: [
        { path: ["url"], old: { kind: "text", text: "a" }, new: { kind: "text", text: "b" }, line: 9 },
        { path: ["headers", "X", "value"], old: null, new: { kind: "text", text: "1" }, line: 14 },
      ],
    },
    { target: { kind: "request", path: "orders/new.yml" }, change: { kind: "added" }, file: "orders/new.yml", name: "New", fields: [] },
    { target: { kind: "request", path: "orders/gone.yml" }, change: { kind: "removed" }, file: "orders/gone.yml", name: "Gone", fields: [] },
    { target: { kind: "request", path: "orders/moved.yml" }, change: { kind: "moved", from: "old.yml" }, file: "orders/moved.yml", name: "Moved", fields: [] },
    { target: { kind: "environment", name: "Local" }, change: { kind: "modified" }, file: "environments/Local.yml", name: "Local", fields: [] },
  ],
  files: [],
};

const comment = (over: Partial<ReviewComment>): ReviewComment => ({
  id: "1",
  author: "grace",
  body: "x",
  created: 10,
  path: null,
  line: null,
  outdated: false,
  ...over,
});

describe("review model", () => {
  it("runs the requests a PR added, changed or moved, not removed ones or environments", () => {
    expect(changedRequestPaths(diff)).toEqual(["orders/list.yml", "orders/new.yml", "orders/moved.yml"]);
  });

  it("maps a comment's line to the field there, also in a sub-folder collection", () => {
    expect(fieldForLine(diff, "orders/list.yml", 14)).toEqual(["headers", "X", "value"]);
    expect(fieldForLine(diff, "api/orders/list.yml", 9)).toEqual(["url"]);
    expect(fieldForLine(diff, "orders/list.yml", 3)).toBeUndefined();
    expect(fieldForLine(diff, "other/list.yml", 9)).toBeUndefined();
    const out = withFields(
      [comment({ id: "a", path: "orders/list.yml", line: 9 }), comment({ id: "b", path: "orders/list.yml", line: 9, outdated: true }), comment({ id: "c" })],
      diff,
    );
    expect(out.map((c) => c.field)).toEqual([["url"], undefined, undefined]);
    expect(withFields([comment({})], null)).toHaveLength(1);
  });

  it("groups comments by file, general ones first, oldest first", () => {
    const groups = groupComments([
      comment({ id: "late", created: 30, path: "a.yml", line: 1 }),
      comment({ id: "general", created: 20 }),
      comment({ id: "early", created: 10, path: "a.yml", line: 2 }),
      comment({ id: "other", created: 15, path: "b.yml", line: 1 }),
    ]);
    expect(groups.map((g) => [g.file, g.comments.map((c) => c.id)])).toEqual([
      [null, ["general"]],
      ["a.yml", ["early", "late"]],
      ["b.yml", ["other"]],
    ]);
  });

  it("offers the device login only for the public hosts and only once the apps are registered", () => {
    const repo = (host: string): ReviewRepo => ({ kind: "github", host, api: "https://x", project: "a/b" });
    expect(deviceLoginOffered(repo("github.com"))).toBe(false);
    expect(deviceLoginOffered(repo("github.com"), true)).toBe(true);
    expect(deviceLoginOffered(repo("git.corp.example"), true)).toBe(false);
  });

  it("says how long ago", () => {
    expect(relativeTime(1000, 1030)).toBe("just now");
    expect(relativeTime(1000, 1000 + 180)).toBe("3 minutes ago");
    expect(relativeTime(1000, 1000 + 3600)).toBe("1 hour ago");
    expect(relativeTime(1000, 1000 + 2 * 86_400)).toBe("2 days ago");
  });
});
