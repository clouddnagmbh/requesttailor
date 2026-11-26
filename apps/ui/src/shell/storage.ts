// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// RequestTailor's per-device settings: the shared storage helper (@tailor-kit/shell) with
// today's key prefix, so what users stored stays readable.

import { createShellStorage } from "@tailor-kit/shell";

export const shellStorage = createShellStorage("rt.shell.");
export const { load, store } = shellStorage;
export { isNumber } from "@tailor-kit/shell";
