// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ReviewApi` of the desktop shell (wave 12, strand E): every method calls its command with
// the arguments the Rust side expects (camelCase, the token only as `setToken`'s argument).

import { describe, expect, it } from "vitest";

import { COMMANDS, type Command } from "./backend";
import { tauriReview } from "./review";

type Call = [Command, Record<string, unknown> | undefined];

describe("tauriReview", () => {
  it("maps every method to its command", async () => {
    const calls: Call[] = [];
    const api = tauriReview(async (cmd, args) => {
      calls.push([cmd, args as Record<string, unknown> | undefined]);
      return null as never;
    });
    await api.repo();
    await api.hosts();
    await api.setHosts([{ host: "git.example.com", kind: "github", api: "https://git.example.com/api/v3" }]);
    await api.hasToken("github.com");
    await api.setToken("github.com", "t0k");
    await api.deleteToken("github.com");
    await api.startLogin("github.com");
    await api.finishLogin("github.com");
    await api.list();
    await api.open(7);
    await api.comments(7);
    await api.comment(7, { path: "a.yml", line: 3, body: "hi" });
    await api.submit(7, { verdict: "approve", body: "" });
    await api.ci(7);
    await api.create({ title: "T", body: "", head: "feature", base: "main", draft: false });
    expect(calls).toEqual([
      ["review_repo", undefined],
      ["review_hosts", undefined],
      ["review_set_hosts", { hosts: [{ host: "git.example.com", kind: "github", api: "https://git.example.com/api/v3" }] }],
      ["review_has_token", { host: "github.com" }],
      ["review_set_token", { host: "github.com", token: "t0k" }],
      ["review_delete_token", { host: "github.com" }],
      ["review_start_login", { host: "github.com" }],
      ["review_finish_login", { host: "github.com" }],
      ["review_list", undefined],
      ["review_open", { number: 7 }],
      ["review_comments", { number: 7 }],
      ["review_comment", { number: 7, comment: { path: "a.yml", line: 3, body: "hi" } }],
      ["review_submit", { number: 7, review: { verdict: "approve", body: "" } }],
      ["review_ci", { number: 7 }],
      ["review_create", { pull: { title: "T", body: "", head: "feature", base: "main", draft: false } }],
    ]);
    const reviewCommands = COMMANDS.filter((c) => c.startsWith("review_"));
    expect(new Set(calls.map(([c]) => c))).toEqual(new Set(reviewCommands));
  });
});
