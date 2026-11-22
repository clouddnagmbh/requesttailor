// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Import in the mock backend, for the import UI's tests. Owner: wave 4, strand B.

import { BackendError, type ImportApi } from "./backend";

export function mockImports(): ImportApi {
  return {
    analyze: async () => {
      throw new BackendError("import", "Import is not available in the mock backend yet.");
    },
    write: async () => null,
    discard: async () => {},
  };
}
