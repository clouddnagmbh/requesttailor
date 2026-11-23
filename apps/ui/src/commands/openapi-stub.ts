// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// OpenAPI wizard interface: a placeholder `OpenApiApi` until each shell implements it in its
// own file (desktop `src/openapi.ts`, web `src/openapi.ts`, mock `mock-openapi.ts`). Reading
// and converting reject with `unsupported`; the run wizard finds nothing to ask, so sending
// is unchanged.

import type { OpenApiApi } from "./backend";
import { BackendError } from "./backend";

const notYet = () => Promise.reject(new BackendError("unsupported", "The OpenAPI wizard is not available in this build yet."));

export function stubOpenApi(): OpenApiApi {
  return {
    fetch: notYet,
    inspect: notYet,
    plan: notYet,
    requirements: async () => null,
    validateResponse: async () => null,
  };
}
