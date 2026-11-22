// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// isomorphic-git uses Node's global `Buffer`; browsers have none. Imported first by
// ./repo.ts, so it runs before isomorphic-git does.

import { Buffer } from "buffer";

const g = globalThis as { Buffer?: unknown };
g.Buffer ??= Buffer;
