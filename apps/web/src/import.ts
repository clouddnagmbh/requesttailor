// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ImportApi` of the web app: the core's `Importer` converts; `write` asks for a folder,
// creates `plan.folder_name` in it, writes the files and opens it. Owner: wave 4, strand B.

import { BackendError, type ImportApi } from "@requesttailor/ui";

import type { WebHost } from "./web-backend";

export function webImports(host: WebHost): ImportApi {
  void host;
  return {
    analyze: async () => {
      throw new BackendError("unsupported", "Import is not available in the web app yet.");
    },
    write: async () => null,
    discard: async () => {},
  };
}
