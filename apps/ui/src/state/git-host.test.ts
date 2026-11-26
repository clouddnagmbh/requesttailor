// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Tailor Kit Authors

// RequestTailor's `GitHost` over `AppState` (seam N6) and its brand strings.

import { gitWord } from "@tailor-kit/git-ui";
import { describe, expect, it } from "vitest";

import { MockBackend } from "../commands/mock";
import { AppState } from "./app.svelte";
import { createGitHost, RT_BRAND } from "./git-host";

describe("brand", () => {
  it("names the Git word for the backup refs after the product", () => {
    expect(gitWord("backup", RT_BRAND)).toBe("refs/requesttailor/backup");
    expect(gitWord("fetch", RT_BRAND)).toBe("fetch");
  });

  it("keeps RequestTailor's literals", () => {
    expect(RT_BRAND).toMatchObject({ productName: "RequestTailor", commitMime: "application/x-requesttailor-commit", refNamespace: "requesttailor", scopeNoun: "collection" });
    expect(RT_BRAND.webRepositoryHint).toBe("In the browser, the repository must be the collection folder itself: the app cannot see the folders above it.");
  });
});

describe("RequestTailor's host over AppState", () => {
  it("reads and acts on the app", async () => {
    const app = new AppState(new MockBackend());
    const host = createGitHost(app);
    expect(host.brand).toBe(RT_BRAND);
    expect(host.backend).toBe(app.backend);
    expect(host.content?.activeOwner).toBeNull();
    expect(host.activeRepository).toBeNull();
    expect(host.dialogOpen).toBe(false);
    host.notifyError("boom");
    expect(app.error).toBe("boom");
    host.show({ kind: "graph" });
    expect(app.view?.view).toEqual({ kind: "git", view: { kind: "graph" } });
    const closed = host.showDialog({ title: "T", component: (() => {}) as never });
    expect(host.dialogOpen).toBe(true);
    app.closeDialog(7);
    expect(await closed).toBe(7);
    app.prefillCommitMessage("msg");
    expect(host.commitPrefill?.text).toBe("msg");
    expect(host.takeCommitPrefill()).toBe("msg");
    expect(host.commitPrefill).toBeNull();
  });

  it("follows the open collection", async () => {
    const app = new AppState(new MockBackend());
    await app.open();
    const host = createGitHost(app);
    expect(host.content?.activeOwner).toBe(app.activeCollection);
    expect(host.cacheScope).toBe(app.activeCollection);
    expect(host.reloadCount).toBe(app.loadCount);
    expect(host.unsaved()).toEqual(app.dirty);
    expect(host.content?.owners.map((o) => o.id)).toContain(app.activeCollection);
    expect(host.content?.tree).toBe(app.tree);
  });
});
