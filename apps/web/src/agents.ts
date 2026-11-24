// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AgentsApi` of the web app (wave 8, strand C). The policy is part of the collection
// (`extensions.requesttailor.agents` in opencollection.yml) and is edited like everything
// else, unsaved until Save (rt-wasm `Workspace.agentPolicy`/`setAgentPolicy`,
// crates/rt-wasm/src/agents_js.rs). Coding agents connect through `rt mcp` on the user's
// machine, never to the browser: nothing waits for approval here and there is no audit log.

import type { AgentCheck, AgentPolicy, AgentsApi, RequestSpec } from "@requesttailor/ui";

import type { Session } from "./web-backend";
import { toBackendError, type CoreWorkspace } from "./wasm";

/** The rt-wasm methods of this strand. */
interface AgentsWorkspace {
  agentPolicy(): AgentPolicy;
  setAgentPolicy(policy: AgentPolicy): void;
  agentCheck(path: string, environment: string | null): AgentCheck;
  agentCheckSpec(spec: RequestSpec, environment: string | null): AgentCheck;
}

function agentsOf(ws: CoreWorkspace): AgentsWorkspace {
  return ws as unknown as AgentsWorkspace;
}

export function webAgents(session: () => Session): AgentsApi {
  const run = async <T>(f: (ws: AgentsWorkspace) => T): Promise<T> => {
    try {
      return f(agentsOf(session().ws));
    } catch (e) {
      throw toBackendError(e);
    }
  };
  return {
    policy: () => run((ws) => ws.agentPolicy()),
    setPolicy: (policy) => run((ws) => ws.setAgentPolicy(policy)),
    check: (path, environment) => run((ws) => ws.agentCheck(path, environment)),
    checkSpec: (spec, environment) => run((ws) => ws.agentCheckSpec(spec, environment)),
    pending: async () => [],
    decide: async (id) => {
      throw toBackendError(new Error(`no pending approval ${id}: agents run through rt mcp, not in the browser`));
    },
    onPending: () => () => {},
    audit: async () => [],
  };
}
