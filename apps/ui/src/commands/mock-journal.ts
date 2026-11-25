// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `JournalApi` of the mock backend (wave 19, strand C): a small model of `rt_app::journal` over
// the mock's own sends, so UI tests see a real journal. The mock records every successful
// writing send while "Track test data" is on (the switch starts off), keeps no bodies, gives
// each entry an id (`mock-<n>`) and a Location, fills `{{journal.*}}` of the request a manual
// revert uses, asks for confirmation in production (name rule of the mock, or the policy flag),
// sends the step and marks the entry reverted. No conflict guard: the mock server has no state.

import { BackendError, type JournalApi, type SendInput } from "./backend";
import { mockEffect } from "./mock-agents";
import type {
  AgentPolicy,
  CompensationAnswer,
  CompensationItem,
  EntryStates,
  JournalEntry,
  JournalQuery,
  JournalSettings,
  PluginCompensationRule,
  PreRead,
  RequestSpec,
  RevertOutcome,
  RevertPlan,
  RevertStep,
  StepOutcome,
  StoredResponse,
} from "./types";

const RING_ENTRIES = 5000;
const MAX_STEPS = 500;
const looksLikeProduction = (name: string) => /prod|prd|\blive\b/i.test(name);

/** What the mock journal uses of the mock backend. */
export interface MockJournalHost {
  getRequest(path: string): Promise<RequestSpec>;
  policy(): Promise<AgentPolicy>;
  /** Sends a request that is not in the collection (a step of a plan). */
  sendSpec(spec: RequestSpec, input: SendInput): Promise<StoredResponse>;
}

export interface MockJournal extends JournalApi {
  /** Called by the mock after each send of the app. */
  record(path: string, spec: RequestSpec, input: SendInput, result: StoredResponse): void;
}

function fill(spec: RequestSpec, entry: JournalEntry): RequestSpec {
  let text = JSON.stringify(spec);
  const values: [string, string | undefined, string][] = [
    ["{{journal.id}}", entry.resource_id, "an id (set an id path in the journal settings)"],
    ["{{journal.location}}", entry.location, "a Location"],
    ["{{journal.etag}}", entry.etag, "an ETag"],
  ];
  for (const [name, value, what] of values) {
    if (!text.includes(name)) continue;
    if (value === undefined) {
      throw new BackendError("shell", `the request uses ${name}, but entry #${entry.id} has no ${what}`);
    }
    // Replace inside JSON strings: escape the value as JSON text.
    text = text.split(name).join(JSON.stringify(value).slice(1, -1));
  }
  return JSON.parse(text) as RequestSpec;
}

export function mockJournal(host: MockJournalHost): MockJournal {
  let settings: JournalSettings = { enabled: false };
  let entries: JournalEntry[] = [];
  let nextId = 1;
  const session = `s-${Date.now().toString(16)}`;

  const isProduction = async (environment: string | null): Promise<boolean> => {
    if (environment === null) return false;
    const policy = await host.policy();
    const rule = policy.environments.find((e) => e.environment === environment);
    return rule ? (rule.production ?? false) : looksLikeProduction(environment);
  };

  const urlOf = (spec: RequestSpec) => spec.url;

  async function stepOf(entryId: number, request: RequestSpec, description: string, compensation?: RevertStep["compensation"]): Promise<RevertStep> {
    return {
      entry_id: entryId,
      state: "planned",
      method: request.method,
      url: urlOf(request),
      effect: mockEffect(request),
      description,
      compensation,
    };
  }

  return {
    record(path, spec, input, result) {
      if (!settings.enabled || mockEffect(spec) !== "write") return;
      const status = result.report.response?.status ?? 0;
      if (status < 200 || status >= 300) return;
      const id = nextId++;
      const resourceId = settings.id_path ? `mock-${id}` : undefined;
      const url = result.report.request?.url ?? spec.url;
      entries.push({
        id,
        session,
        at: Date.now(),
        environment: input.environment ?? undefined,
        request_path: path,
        method: spec.method,
        url,
        status,
        location: spec.method.toUpperCase() === "POST" ? `${url.split("?")[0]}/${resourceId ?? id}` : undefined,
        etag: `"v${id}"`,
        resource_id: resourceId,
      });
      if (entries.length > RING_ENTRIES) entries = entries.slice(entries.length - RING_ENTRIES);
    },

    async settings() {
      return { ...settings };
    },
    async setSettings(next) {
      const path = next.id_path?.trim();
      if (path && !path.startsWith("var:") && !path.startsWith("$")) {
        throw new BackendError("shell", `the id path ${JSON.stringify(path)} is not a valid JSONPath (for example $.id)`);
      }
      settings = { enabled: next.enabled ?? false, id_path: path || undefined };
    },
    async entries(query: JournalQuery) {
      const limit = query.limit && query.limit > 0 ? query.limit : 200;
      const matching = entries
        .filter((e) => (query.before === undefined ? true : e.id < query.before))
        .filter((e) => (query.session === undefined ? true : e.session === query.session))
        .filter((e) => (query.only_open ? e.reverted_at === undefined : true))
        .reverse();
      return { entries: matching.slice(0, limit), total: matching.length };
    },
    async clear() {
      entries = [];
    },

    async planManual(revert) {
      const entry = entries.find((e) => e.id === revert.entry_id);
      if (!entry) throw new BackendError("shell", `journal entry #${revert.entry_id} not found`);
      const spec = fill(await host.getRequest(revert.request_path), entry);
      const step = await stepOf(entry.id, spec, `Send ${spec.name || revert.request_path} with the values of #${entry.id}`, {
        kind: "manual",
        request: spec,
        description: `Send ${spec.name || revert.request_path} with the values of #${entry.id} (${entry.method} ${entry.url})`,
      });
      return { environment: entry.environment ?? null, production: await isProduction(entry.environment ?? null), steps: [step] };
    },

    async planCompensations(items: CompensationItem[]) {
      const found = items.map((i) => {
        const entry = entries.find((e) => e.id === i.entry_id);
        if (!entry) throw new BackendError("shell", `journal entry #${i.entry_id} not found`);
        return { entry, answer: i.answer as CompensationAnswer };
      });
      const environment = found[0]?.entry.environment ?? null;
      if (found.some((f) => (f.entry.environment ?? null) !== environment)) {
        throw new BackendError("shell", "the entries are from different environments; revert one environment at a time");
      }
      found.sort((a, b) => b.entry.id - a.entry.id);
      const steps: RevertStep[] = [];
      for (const { entry, answer } of found.slice(0, MAX_STEPS)) {
        if (entry.reverted_at !== undefined) {
          steps.push({ entry_id: entry.id, state: "not-reversible", method: entry.method, url: entry.url, effect: "write", description: `${entry.method} ${entry.url}`, reason: "already reverted" });
        } else if (answer.kind === "not-reversible") {
          steps.push({ entry_id: entry.id, state: "not-reversible", method: entry.method, url: entry.url, effect: "write", description: `${entry.method} ${entry.url}`, reason: answer.reason });
        } else {
          steps.push(await stepOf(entry.id, answer.compensation.request, answer.compensation.description, answer.compensation));
        }
      }
      return { environment, production: await isProduction(environment), steps, truncated: found.length > MAX_STEPS } satisfies RevertPlan;
    },

    async execute(plan, options): Promise<RevertOutcome> {
      if (plan.environment !== null && (await isProduction(plan.environment)) && !options.production_confirmed) {
        const n = plan.steps.filter((s) => s.state === "planned").length;
        throw new BackendError("shell", `Revert ${n} change${n === 1 ? "" : "s"} in PRODUCTION? It needs the user's explicit confirmation.`);
      }
      const steps: StepOutcome[] = [];
      for (const step of plan.steps) {
        if (step.state !== "planned" || !step.compensation) {
          steps.push({ entry_id: step.entry_id, result: "skipped-not-reversible", message: step.reason ?? "not reversible" });
          continue;
        }
        try {
          const stored = await host.sendSpec(step.compensation.request, { environment: plan.environment, vars: [] });
          const status = stored.report.response?.status ?? 0;
          if (status >= 200 && status < 300) {
            const entry = entries.find((e) => e.id === step.entry_id);
            if (entry) entry.reverted_at = Date.now();
            steps.push({ entry_id: step.entry_id, result: "reverted", message: `HTTP ${status}` });
          } else {
            steps.push({ entry_id: step.entry_id, result: "failed", message: `HTTP ${status || "no answer"}` });
          }
        } catch (e) {
          steps.push({ entry_id: step.entry_id, result: "failed", message: e instanceof Error ? e.message : String(e) });
        }
      }
      return { steps };
    },

    async pluginRules(): Promise<PluginCompensationRule[]> {
      return [];
    },
    async preRead(request): Promise<PreRead> {
      if (mockEffect(request) !== "read") throw new BackendError("shell", "a read before a write must be a reading request");
      return { status: 200, body: "{}", etag: null };
    },
    async states(entryIds): Promise<EntryStates[]> {
      return entryIds.map((entry_id) => ({ entry_id, before: null, after: null }));
    },
  };
}
