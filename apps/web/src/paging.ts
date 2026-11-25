// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `PagingApi` of the web app (wave 18, strand C), over rt-wasm's `Workspace.pagingRule`,
// `pagingSetRule` and `pagingRun` (crates/rt-wasm/src/paging_js.rs). Every page goes out like a
// single send (companion when it is on, the same services, so the send guard answers for each
// page), with the environment's secret values that never leave the core. The paging rules of the
// active plugins are read from their manifests here, because the plugins live on this side.

import type {
  EffectivePaging,
  PagedResult,
  PagingApi,
  PagingContribution,
  PagingRuleScope,
  PagingRun,
  PagingSpec,
  PluginInfo,
} from "@requesttailor/ui";

import type { RunContext } from "./runner";
import type { CompanionCredentials, CoreSendInput } from "./wasm";
import { toBackendError } from "./wasm";
import type { CoreSendServices } from "./send-services";
import type { Session } from "./web-backend";

/** What rt-wasm's `PluginPaging` reads. */
interface PluginPaging {
  plugin: string;
  contribution: PagingContribution;
}

/** The paging part of rt-wasm's `Workspace`. */
export interface CorePagingWorkspace {
  pagingRule(path: string, responseId: number | null, plugins: PluginPaging[]): EffectivePaging | null;
  pagingSetRule(path: string, spec: PagingSpec | null, scope: PagingRuleScope): void;
  pagingRun(
    run: PagingRun,
    plugins: PluginPaging[],
    input: CoreSendInput,
    companion: CompanionCredentials | null,
    services: CoreSendServices | null,
    cancelled: () => boolean,
  ): Promise<PagedResult>;
}

export interface PagingHost {
  session(): Session;
  /** What a send needs (secret values, companion, services), as for the runner. */
  sendContext(input: PagingRun["input"]): Promise<RunContext>;
  /** The installed plugins (their manifests carry `contributes.paging`). */
  plugins(): Promise<PluginInfo[]>;
}

/** The rules of the enabled plugins. */
export async function pluginPaging(plugins: () => Promise<PluginInfo[]>): Promise<PluginPaging[]> {
  const list = await plugins().catch(() => [] as PluginInfo[]);
  return list
    .filter((p) => p.enabled)
    .flatMap((p) => (p.manifest.contributes.paging ?? []).map((contribution) => ({ plugin: p.manifest.id, contribution })));
}

export function webPaging(host: PagingHost): PagingApi {
  const ws = () => host.session().ws as unknown as CorePagingWorkspace;
  return {
    rule: async (path, responseId) => {
      try {
        return ws().pagingRule(path, responseId ?? null, await pluginPaging(host.plugins));
      } catch (e) {
        throw toBackendError(e);
      }
    },
    setRule: async (path, spec, scope) => {
      try {
        ws().pagingSetRule(path, spec, scope);
      } catch (e) {
        throw toBackendError(e);
      }
    },
    run: async (run) => {
      try {
        const context = await host.sendContext(run.input);
        return await ws().pagingRun(run, await pluginPaging(host.plugins), context.input, context.companion ?? null, context.services, () => false);
      } catch (e) {
        throw toBackendError(e);
      }
    },
  };
}
