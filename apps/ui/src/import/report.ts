// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Pure helpers of the import dialog: format names, loss groups, the copyable report text,
// and turning picked files or pasted text into `ImportSource`s. Owner: wave 4, strand B.

import type { ImportSource } from "../commands/backend";
import type { ImportFormat, ImportPlan, Loss, LossKind } from "../commands/types";

export const FORMATS: { value: ImportFormat; label: string }[] = [
  { value: "postman", label: "Postman (collection v2.0/v2.1, environments, globals)" },
  { value: "insomnia", label: "Insomnia (export v4, collection v5)" },
  { value: "openapi", label: "OpenAPI 3.0/3.1/3.2" },
  { value: "curl", label: "cURL commands" },
  { value: "har", label: "HAR 1.2 (browser or proxy recording)" },
  { value: "http", label: ".http / .rest (VS Code REST Client, JetBrains)" },
];

const FORMAT_NAMES: Record<ImportFormat, string> = {
  postman: "Postman",
  insomnia: "Insomnia",
  openapi: "OpenAPI",
  curl: "cURL",
  har: "HAR",
  http: ".http",
  plugin: "Plugin",
};

export function formatName(f: ImportFormat): string {
  return FORMAT_NAMES[f];
}

/** In the order the report shows them; mirrors `LossKind::label` in rt-format. */
export const KIND_LABELS: Record<LossKind, string> = {
  script: "Scripts",
  test: "Tests",
  auth: "Auth and credentials",
  body: "Bodies",
  variable: "Variables and secrets",
  setting: "Settings",
  documentation: "Documentation",
  example: "Examples",
  protocol: "Other protocols",
  other: "Other",
};

export interface LossGroup {
  kind: LossKind;
  label: string;
  losses: Loss[];
  dropped: number;
  approximated: number;
}

export function groupLosses(losses: Loss[]): LossGroup[] {
  return (Object.keys(KIND_LABELS) as LossKind[])
    .map((kind) => {
      const group = losses.filter((l) => l.kind === kind);
      return {
        kind,
        label: KIND_LABELS[kind],
        losses: group,
        dropped: group.filter((l) => l.effect === "dropped").length,
        approximated: group.filter((l) => l.effect === "approximated").length,
      };
    })
    .filter((g) => g.losses.length > 0);
}

const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

export function summary(plan: ImportPlan): string {
  const r = plan.report;
  return [
    count(r.requests, "request", "requests"),
    count(r.folders, "folder", "folders"),
    count(r.environments, "environment", "environments"),
  ].join(", ");
}

/** The report as plain text, for "Copy report" (an issue, a migration note). */
export function reportText(plan: ImportPlan, sources: string[]): string {
  const r = plan.report;
  const lines = [
    `Import of ${sources.join(", ")} (${formatName(r.format)}) as "${plan.collection_name}"`,
    summary(plan),
    "",
  ];
  if (r.losses.length === 0) {
    lines.push("Nothing was lost: everything was imported exactly.");
    return lines.join("\n") + "\n";
  }
  const dropped = r.losses.filter((l) => l.effect === "dropped").length;
  lines.push(`Loss report: ${r.losses.length} (${dropped} dropped, ${r.losses.length - dropped} approximated)`);
  for (const g of groupLosses(r.losses)) {
    lines.push("", `${g.label} (${g.losses.length})`);
    for (const l of g.losses) lines.push(`  ${l.effect.padEnd(12)}  ${l.location}: ${l.detail}`);
  }
  return lines.join("\n") + "\n";
}

export async function readFiles(files: Iterable<File>): Promise<ImportSource[]> {
  const out: ImportSource[] = [];
  for (const f of files) out.push({ name: f.name, data: new Uint8Array(await f.arrayBuffer()) });
  return out;
}

/** Pasted text (cURL commands, `.http` requests, a whole JSON/YAML export). */
export function pasted(text: string): ImportSource {
  const t = text.trimStart();
  // A pasted `.http` block is recognised by its content; the name only hints at it.
  const http = /^(###|(GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS) \S)/m.test(t) && !t.startsWith("curl");
  return { name: http ? "pasted.http" : "pasted.txt", data: new TextEncoder().encode(text) };
}
