// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `GitApi` of the web app: isomorphic-git over the collection's directory handle; network
// (push, pull, clone) through the companion, since browsers block Git hosts (CORS). Diffs come
// from the core (`textDiff`), so they match the desktop app. Owner: wave 4, strand A2.
// Until then: "not a repository", everything else unsupported.

import { BackendError, type GitApi, type GitStatus } from "@requesttailor/ui";

import type { WebHost } from "../web-backend";

const unsupported = async (): Promise<never> => {
  throw new BackendError("unsupported", "Git is not available in the web app yet.");
};

export function webGit(host: WebHost): GitApi {
  void host;
  return {
    status: async (): Promise<GitStatus> => ({
      repo: false,
      branch: null,
      upstream: null,
      ahead: 0,
      behind: 0,
      files: [],
      merging: false,
    }),
    diff: unsupported,
    commit: unsupported,
    log: async () => [],
    branches: async () => [],
    checkout: unsupported,
    createBranch: unsupported,
    push: unsupported,
    pull: unsupported,
    conflict: unsupported,
    resolve: unsupported,
    init: unsupported,
    clone: unsupported,
    identity: async () => null,
    setIdentity: unsupported,
    hasCredentials: async () => false,
    setCredentials: unsupported,
    deleteCredentials: unsupported,
  };
}
