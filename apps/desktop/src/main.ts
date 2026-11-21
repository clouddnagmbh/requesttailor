// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Desktop shell entry: the shared UI on the TauriBackend.

import { invoke } from "@tauri-apps/api/core";
import { startApp } from "@requesttailor/ui";

import { TauriBackend, type Invoke } from "./backend";

const backend = new TauriBackend(invoke as Invoke);

startApp(document.getElementById("app")!, backend);
