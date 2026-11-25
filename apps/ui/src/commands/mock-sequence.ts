// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `SequenceApi` of the mock backend (wave 19, strand G): a small fixed diagram for UI tests (the
// Rust core is not in the browser here), the same shapes the core answers, and a TypeScript
// copy of the Mermaid text rules just for the preview. `?sequence=big` answers 2 000 messages
// between 8 participants (the scroll budget), `?sequence=500` that many. Nothing here is read from a real run.

import type { SequenceApi } from "./backend";
import type { SequenceMessage, SequenceModel, SequenceNote, SequenceOptions, SequenceParticipant, SequenceSource, SequenceText } from "./types";

const MAX_LABEL = 80;

/** The same escaping as `rt_app::sequence::mermaid::escape`. */
export function escapeMermaid(text: string): string {
  let out = "";
  let count = 0;
  for (const c of text.trim()) {
    if (count >= MAX_LABEL) {
      out += "…";
      break;
    }
    count++;
    switch (c) {
      case ";": out += "#59;"; break;
      case "#": out += "#35;"; break;
      case ":": out += "#58;"; break;
      case "%": out += "#37;"; break;
      case "<": out += "#lt;"; break;
      case ">": out += "#gt;"; break;
      case "`": out += "#96;"; break;
      default: out += /[\u0000-\u001f\u007f]/.test(c) ? " " : c;
    }
  }
  return out.trim() === "" ? "-" : out;
}

function demo(source: SequenceSource, options: SequenceOptions): SequenceModel {
  const participants: SequenceParticipant[] = [
    { id: "client", label: "Client", kind: "client" },
    { id: "api.example.com", label: "api.example.com", kind: "server" },
    { id: "auth.example.com", label: "auth.example.com", kind: "server" },
  ];
  const q = options.include_query ? "?limit=5" : "";
  const messages: SequenceMessage[] = [
    { kind: "request", from: "client", to: "auth.example.com", label: "POST /token", request_path: "health.yml" },
    { kind: "response", from: "auth.example.com", to: "client", label: "200 (38 ms)", request_path: "health.yml" },
    { kind: "request", from: "client", to: "api.example.com", label: `GET /orders${q}`, request_path: "users/list-users.yml" },
    { kind: "response", from: "api.example.com", to: "client", label: "200 (41 ms)", request_path: "users/list-users.yml" },
    { kind: "request", from: "client", to: "api.example.com", label: "GET /orders/{id}", request_path: "users/create-user.yml" },
    { kind: "response", from: "api.example.com", to: "client", label: "404 (12 ms)", request_path: "users/create-user.yml" },
  ];
  const notes: SequenceNote[] = [
    { after: 5, anchor: { kind: "over", participant: "client" }, text: "1 of 2 tests failed: finds the order" },
  ];
  if (options.include_values) notes.push({ after: 3, anchor: { kind: "over", participant: "api.example.com" }, text: "response: id=42, status=open" });
  const groups = source.kind === "pages" ? [{ kind: "loop", label: "3 pages", from: 2, to: 5 }] : [];
  if (source.kind === "mcp") {
    participants.unshift({ id: "agent:claude", label: "claude", kind: "agent" });
  }
  return { participants, messages, notes, groups, omitted: 0 };
}

/** `count` messages (a multiple of 2) between 8 participants, for the budgets; 2 000 by default. */
export function bigModel(count = 2000, options: SequenceOptions = {}): SequenceModel {
  const participants: SequenceParticipant[] = [{ id: "client", label: "Client", kind: "client" }];
  for (let i = 1; i < 8; i++) participants.push({ id: `h${i}.example.com`, label: `h${i}.example.com`, kind: "server" });
  const messages: SequenceMessage[] = [];
  for (let i = 0; i < count / 2; i++) {
    const host = `h${(i % 7) + 1}.example.com`;
    const path = `r${i}.yml`;
    messages.push({ kind: "request", from: "client", to: host, label: `GET /items/${i % 13}${options.include_query ? `?page=${i % 5}` : ""}`, request_path: path });
    messages.push({ kind: "response", from: host, to: "client", label: `200 (${10 + (i % 90)} ms)`, request_path: path });
  }
  const notes: SequenceNote[] = [];
  for (let i = 1; i < count; i += 40) notes.push({ after: i, anchor: { kind: "over", participant: "client" }, text: `note ${i}` });
  return { participants, messages, notes, groups: [], omitted: 0 };
}

export function mermaidOf(model: SequenceModel): string {
  const alias = new Map<string, string>();
  let out = "sequenceDiagram\n";
  model.participants.forEach((p, i) => {
    alias.set(p.id, `P${i}`);
    out += `    participant P${i} as ${escapeMermaid(p.label)}\n`;
  });
  const name = (id: string): string => alias.get(id) ?? "P0";
  const notes = new Map<number, SequenceNote[]>();
  for (const n of model.notes ?? []) notes.set(n.after, [...(notes.get(n.after) ?? []), n]);
  model.messages.forEach((m, i) => {
    out += `    ${name(m.from)}${m.kind === "request" ? "->>" : "-->>"}${name(m.to)}: ${escapeMermaid(m.label)}\n`;
    for (const n of notes.get(i) ?? []) {
      const who = n.anchor.kind === "over" ? name(n.anchor.participant) : `${name(n.anchor.from)},${name(n.anchor.to)}`;
      out += `    Note over ${who}: ${escapeMermaid(n.text)}\n`;
    }
  });
  return out;
}

export function mockSequence(): SequenceApi {
  const param = typeof location !== "undefined" ? new URLSearchParams(location.search).get("sequence") : null;
  const big = param === "big" ? 2000 : param !== null && /^\d+$/.test(param) ? Number(param) : 0;
  let cleared = false;
  return {
    model: async (source, options) => (big ? bigModel(big, options) : cleared && source.kind === "session" ? { participants: [{ id: "client", label: "Client", kind: "client" }], messages: [] } : demo(source, options)),
    render: async (model, _options): Promise<SequenceText> => {
      const mermaid = mermaidOf(model);
      return { mermaid, markdown: `\`\`\`mermaid\n${mermaid.trimEnd()}\n\`\`\`\n` };
    },
    clear: async () => {
      cleared = true;
    },
  };
}
