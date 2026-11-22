// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Script trust in the web app (wave 7, strand A owns this file): which collections the user
// trusts to run scripts, kept in browser storage per collection id. Interface stub: nothing
// is trusted and no collection reports scripts yet.

import type { ScriptTrust } from "@requesttailor/ui";

import type { WebHost } from "./web-backend";

export interface WebScriptTrust {
  trust(): Promise<ScriptTrust>;
  setTrusted(trusted: boolean): Promise<void>;
}

export function webScriptTrust(_host: Pick<WebHost, "session">): WebScriptTrust {
  return {
    trust: async () => ({ has_scripts: false, trusted: false }),
    setTrusted: async () => {},
  };
}
