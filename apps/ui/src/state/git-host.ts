// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// RequestTailor's `GitHost` (seam N6): the Git views talk to this, not to `AppState`. Reads are
// getters, so they stay reactive; the content part gives the views collections.

import MergeFields from "../merge/MergeFields.svelte";
import SemanticDiff from "../diff/SemanticDiff.svelte";
import { gitApiCommands } from "../git/api-commands";
import type { GitBrand, GitContent, GitHost, ProvidedGitExtensions } from "@tailor-kit/git-ui";
import PullView from "../review/PullView.svelte";
import { hintStore } from "../shell/hint-store";
import { shellStorage } from "../shell/storage";
import type { AppState } from "./app.svelte";

export const RT_BRAND: GitBrand = {
  productName: "RequestTailor",
  commitMime: "application/x-requesttailor-commit",
  refNamespace: "requesttailor",
};

/** `extensions` reads the shell's extensions when the views ask: the shell provides them after the host. */
export function createGitHost(app: AppState, extensions: () => ProvidedGitExtensions | null = () => null): GitHost {
  const content: GitContent = {
    get activeOwner() {
      return app.activeCollection;
    },
    get owners() {
      return app.collections;
    },
    activate: (owner) => app.activate(owner),
    get tree() {
      return app.tree;
    },
    get activeFile() {
      return app.active;
    },
    diffs: app.backend.diffs,
    merges: app.backend.merges,
    save: () => app.save(),
    adopt: (info) => app.adopt(info),
    components: { SemanticDiff, MergeFields, PullView },
  };
  const host: GitHost = {
    backend: app.backend,
    brand: RT_BRAND,
    get activeRepository() {
      return app.activeRepository;
    },
    get repositories() {
      return app.repositories;
    },
    get reloadCount() {
      return app.loadCount;
    },
    get cacheScope() {
      return app.activeCollection;
    },
    get commitPrefill() {
      return app.commitPrefill;
    },
    get dialogOpen() {
      return app.dialog !== null;
    },
    unsaved: () => app.dirty,
    reload: () => app.reload(),
    show: (view) => app.show({ kind: "git", view }),
    showDialog: (dialog) => app.showDialog(dialog),
    takeCommitPrefill: () => app.takeCommitPrefill(),
    adoptRepository: (entry) => app.adoptRepository(entry),
    notifyError: (message) => {
      app.error = message;
    },
    commands: (views) => gitApiCommands(app, host, views),
    get extensions() {
      return extensions();
    },
    storage: shellStorage,
    hints: hintStore,
    content,
  };
  return host;
}
