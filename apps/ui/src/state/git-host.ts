// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// RequestTailor's `GitHost` (seam N6): the Git views talk to this, not to `AppState`. Reads are
// getters, so they stay reactive; the content part gives the views collections.

import MergeFields from "../merge/MergeFields.svelte";
import SemanticDiff from "../diff/SemanticDiff.svelte";
import { gitApiCommands } from "../git/api-commands";
import type { GitBrand, GitContent, GitContentTexts, GitHost, ProvidedGitExtensions } from "@tailor-kit/git-ui";
import PullView from "../review/PullView.svelte";
import { hintStore } from "../shell/hint-store";
import { shellStorage } from "../shell/storage";
import type { RepositoryEntry } from "../commands/types";
import type { AppState } from "./app.svelte";

export const RT_BRAND: GitBrand = {
  productName: "RequestTailor",
  commitMime: "application/x-requesttailor-commit",
  refNamespace: "requesttailor",
  scopeNoun: "collection",
  webRepositoryHint: "In the browser, the repository must be the collection folder itself: the app cannot see the folders above it.",
};

const TEXTS: GitContentTexts = {
  ownedTag: "API",
  ownedTagTitle: "A collection file: shown field by field",
  openOwner: "Open its collection for the field view",
  openOwnerTestId: "changes-open-collection",
  ownedFileTitle: "A file of a collection: the app shows what changed in its requests",
  otherOwnerBefore: "This file belongs to ",
  otherOwnerAfter: ". Make it the active collection to see what changed in its requests.",
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
    ownerOf: (file) => (file as { collection?: string }).collection,
    texts: TEXTS,
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
    // The components take RequestTailor's own (narrower) target types.
    components: {
      SemanticDiff: SemanticDiff as unknown as GitContent["components"]["SemanticDiff"],
      MergeFields: MergeFields as unknown as GitContent["components"]["MergeFields"],
      PullView,
    },
  };
  let lastProvided: ProvidedGitExtensions | null = null;
  let lastWrapped: ProvidedGitExtensions | null = null;
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
    // The backend returns the full entry, `collections` included.
    adoptRepository: (entry) => app.adoptRepository(entry as RepositoryEntry),
    notifyError: (message) => {
      app.error = message;
    },
    commands: (views) => gitApiCommands(app, host, views),
    get extensions() {
      const provided = extensions();
      if (!provided) return null;
      if (provided !== lastProvided) {
        lastProvided = provided;
        // Extensions read `collection` on a range target; the kit's views only know `owner`.
        lastWrapped = { ...provided, rangeTarget: (t) => ({ ...t, collection: t.owner }) as typeof t };
      }
      return lastWrapped;
    },
    storage: shellStorage,
    hints: hintStore,
    content,
  };
  return host;
}
