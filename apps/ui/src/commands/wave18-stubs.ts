// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18 interface: placeholder APIs that reject with `unsupported` until a strand replaces
// them in its own file (mock-*.ts here, the shells' own files in apps/web and apps/desktop).
// The placeholders of a group are removed by the integrator, not by the strand.

import type {
  EnvCompareApi,
  InspectApi,
  PagingApi,
  InboxApi,
  LintApi,
  ViewsApi,
  CatalogueApi,
  ContractApi,
} from "./backend";
import { BackendError } from "./backend";

function notYet(feature: string, strand: string): () => Promise<never> {
  return () => Promise.reject(new BackendError("unsupported", `${feature} is not implemented yet (wave 18, strand ${strand}).`));
}

export function stubEnvCompare(): EnvCompareApi {
  const no = notYet("Comparing environments", "A");
  return { run: no };
}

export function stubInspect(): InspectApi {
  const no = notYet("Response checks", "B");
  return { jwt: no, tokens: no, checks: no, ignoreCheck: no };
}

export function stubPaging(): PagingApi {
  const no = notYet("Paging", "C");
  return { rule: no, setRule: no, run: no };
}

export function stubInbox(): InboxApi {
  const no = notYet("The webhook inbox", "E");
  return { settings: no, setSettings: no, start: no, stop: no, status: no, events: no, get: no, clear: no, makeRequest: no };
}

export function stubLint(): LintApi {
  const no = notYet("Lint", "F");
  return { run: no, settings: no, setSettings: no, ignore: no };
}

export function stubViews(): ViewsApi {
  const no = notYet("Saved views", "G");
  return { list: no, save: no, remove: no, apply: no, languages: no };
}

export function stubCatalogue(): CatalogueApi {
  const no = notYet("The plugin catalogue", "I");
  return { sources: no, addSource: no, removeSource: no, refresh: no, plan: no, install: no, installed: no, checkUpdates: no };
}

/** The placeholder for `ContractApi.schemaChanges` (strand H). */
export function stubSchemaChanges(): ContractApi["schemaChanges"] {
  return notYet("GraphQL and Protobuf contracts", "H");
}
