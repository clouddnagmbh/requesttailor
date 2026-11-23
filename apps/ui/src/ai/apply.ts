// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Pure helpers of the AI assistant's UI (wave 8, strand E): when a response counts as failed,
// the folders a request can be created in, and how proposals become unsaved edits.

import type { AiAnswer, AiConfig, RequestSpec, StoredResponse, TreeNode } from "../commands/types";

/** Whether `result` is worth explaining: an error status, or a failed test or assertion. */
export function failed(result: StoredResponse | undefined): boolean {
  if (!result || result.response_id == null) return false;
  const r = result.report;
  if ((r.response?.status ?? 0) >= 400) return true;
  return (r.tests ?? []).some((t) => !t.passed) || (r.assertions ?? []).some((a) => !a.passed);
}

/** Folder paths of `tree`, root (`""`) first, in tree order. */
export function folders(tree: TreeNode | null): { path: string; label: string }[] {
  const out = [{ path: "", label: "(collection root)" }];
  const walk = (node: TreeNode, labels: string[]) => {
    if (node.kind !== "folder") return;
    for (const child of node.children) {
      if (child.kind !== "folder") continue;
      const label = [...labels, child.name];
      out.push({ path: child.path, label: label.join(" / ") });
      walk(child, label);
    }
  };
  if (tree) walk(tree, []);
  return out;
}

/** The folder of a request path (`""` for the root). */
export function folderOf(path: string | null): string {
  if (!path) return "";
  const i = path.lastIndexOf("/");
  return i < 0 ? "" : path.slice(0, i);
}

/** The created request (`base`, with its name and position) with the proposal's content.
 *  Auth stays as created (inherit). */
export function withProposal(base: RequestSpec, proposal: RequestSpec): RequestSpec {
  return {
    ...base,
    method: proposal.method,
    url: proposal.url,
    headers: proposal.headers,
    params: proposal.params,
    body: proposal.body,
  };
}

/** `spec` with the suggested tests added: the tests script appended to the existing one,
 *  assertions appended unless the same one is there already. */
export function withSuggestedTests(spec: RequestSpec, answer: AiAnswer): RequestSpec {
  const scripts = [...spec.scripts];
  for (const s of answer.scripts ?? []) {
    const at = scripts.findIndex((x) => x.phase === s.phase);
    if (at < 0) scripts.push({ ...s });
    else {
      const current = scripts[at]!.code.trimEnd();
      scripts[at] = { ...scripts[at]!, code: current === "" ? s.code : `${current}\n\n${s.code}` };
    }
  }
  const same = (a: { expression: string; operator: string; value: string }, b: typeof a) =>
    a.expression === b.expression && a.operator === b.operator && a.value === b.value;
  const assertions = [...spec.assertions];
  for (const a of answer.assertions ?? []) if (!assertions.some((x) => same(x, a))) assertions.push({ ...a });
  return { ...spec, scripts, assertions };
}

/** Whether the assistant can be used with `config` (Anthropic also needs a key). */
export function usable(config: AiConfig | null, hasKey: boolean): boolean {
  if (!config || config.model.trim() === "") return false;
  if (config.provider === "openai-compatible") return (config.base_url ?? "").trim() !== "";
  return hasKey;
}

/** A readable message for a failed `ask`. */
export function askError(e: unknown): string {
  const kind = typeof e === "object" && e !== null && "kind" in e ? String((e as { kind: unknown }).kind) : "";
  const message = e instanceof Error ? e.message : String(e);
  switch (kind) {
    case "not-configured":
      return "The AI assistant is not set up. Open its settings first.";
    case "no-key":
      return "No API key is set for this provider. Add one in the AI assistant settings.";
    case "opted-out":
      return "AI is off for this collection.";
    default:
      return message;
  }
}
