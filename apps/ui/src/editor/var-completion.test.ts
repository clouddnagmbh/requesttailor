// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The `{{` completion on CodeMirror state (no DOM): when it opens, what it lists, what Enter
// inserts, and how fast it answers for 500 variables.

import { CompletionContext, type Completion, type CompletionResult } from "@codemirror/autocomplete";
import { EditorSelection, EditorState, type TransactionSpec } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { describe, expect, it } from "vitest";

import type { VariableScope } from "../commands/types";
import { varCompletionSource } from "./var-completion";
import type { VarSource } from "./var-source";

const scope: VariableScope = {
  variables: [
    {
      name: "baseUrl",
      value: "http://c",
      source: { layer: "collection" },
      secret: false,
      external: null,
      overridden: true,
    },
    {
      name: "baseUrl",
      value: "http://local",
      source: { layer: "environment", name: "Local" },
      secret: false,
      external: null,
      overridden: false,
    },
    {
      name: "token",
      value: null,
      source: { layer: "environment", name: "Local" },
      secret: true,
      external: null,
      overridden: false,
    },
    {
      name: "userId",
      value: "42",
      source: { layer: "folder", path: "users" },
      secret: false,
      external: null,
      overridden: false,
    },
  ],
  undefined: ["missing"],
};

function source(s: VariableScope | null, loaded: VariableScope = scope): VarSource {
  return { cached: () => s, load: async () => loaded, targets: () => [], create: async () => undefined };
}

/** Text with `|` as the cursor. */
function state(text: string): EditorState {
  const pos = text.indexOf("|");
  return EditorState.create({ doc: text.replace("|", ""), selection: EditorSelection.cursor(pos) });
}

function complete(text: string, vars = source(scope), explicit = false) {
  const s = state(text);
  return varCompletionSource(vars)(new CompletionContext(s, s.selection.main.head, explicit));
}

const labels = (r: CompletionResult | Promise<CompletionResult | null> | null) =>
  (r as CompletionResult | null)?.options.map((o) => o.label) ?? null;

/** A stand-in for the view that `apply` dispatches to. */
function fakeView(s: EditorState) {
  const view = {
    state: s,
    dispatch(spec: TransactionSpec) {
      view.state = view.state.update(spec).state;
    },
  };
  return view;
}

function accept(text: string, pick: (o: Completion) => boolean): string {
  const s = state(text);
  const result = varCompletionSource(source(scope))(
    new CompletionContext(s, s.selection.main.head, false),
  ) as CompletionResult;
  const option = result.options.find(pick)!;
  const view = fakeView(s);
  (option.apply as (v: EditorView, c: Completion, from: number, to: number) => void)(
    view as unknown as EditorView,
    option,
    result.from,
    result.to ?? s.selection.main.head,
  );
  const doc = view.state.doc.toString();
  const head = view.state.selection.main.head;
  return `${doc.slice(0, head)}|${doc.slice(head)}`;
}

describe("the {{ completion source", () => {
  it("opens right after {{ with every variable, undefined names and Create last", () => {
    expect(complete("x|")).toBeNull();
    expect(complete("{|")).toBeNull();
    const r = complete("{{|") as CompletionResult;
    expect(labels(r)).toEqual(["baseUrl", "baseUrl", "token", "userId", "missing", "Create variable…"]);
    expect(r.from).toBe(2);
    expect(r.filter).toBe(false);
  });

  it("filters while typing, prefix matches first, and offers to create an unknown name", () => {
    expect(labels(complete("{{us|"))).toEqual(["userId", "Create variable “us”…"]);
    expect(labels(complete("{{rl|"))).toEqual(["baseUrl", "baseUrl", "Create variable “rl”…"]);
    expect(labels(complete("{{token|"))).toEqual(["token"]);
    expect(labels(complete("{{zz|"))).toEqual(["Create variable “zz”…"]);
  });

  it("stays closed after }}, inside whitespace, and opens on Ctrl+Space inside an open {{", () => {
    expect(complete("{{a}} |")).toBeNull();
    expect(complete("{{a |")).toBeNull();
    expect(labels(complete("{{ba|seUrl}}", source(scope), true))).toContain("baseUrl");
  });

  it("answers from the cached scope synchronously, else after loading", async () => {
    expect(complete("{{|")).not.toBeInstanceOf(Promise);
    const pending = complete("{{|", source(null, scope));
    expect(pending).toBeInstanceOf(Promise);
    expect(labels(await pending)).toContain("userId");
  });

  it("lists 500 variables in well under 50 ms", () => {
    const many: VariableScope = {
      variables: Array.from({ length: 500 }, (_, i) => ({
        name: `variable${i}`,
        value: `value-${i}`,
        source: { layer: "environment" as const, name: "Local" },
        secret: false,
        external: null,
        overridden: false,
      })),
      undefined: [],
    };
    const vars = source(many);
    complete("{{|", vars);
    const start = performance.now();
    const r = complete("{{|", vars) as CompletionResult;
    const filtered = complete("{{variable4|", vars) as CompletionResult;
    expect(performance.now() - start).toBeLessThan(50);
    expect(r.options).toHaveLength(501);
    expect(filtered.options[0].label).toBe("variable4");
  });
});

describe("accepting a variable", () => {
  it("inserts name}} and puts the cursor after it", () => {
    expect(accept("{{|", (o) => o.label === "userId")).toBe("{{userId}}|");
    expect(accept("GET {{ba|/users", (o) => o.label === "baseUrl")).toBe("GET {{baseUrl}}|/users");
  });

  it("completes an existing }} without doubling it", () => {
    expect(accept("{{ba|}}", (o) => o.label === "baseUrl")).toBe("{{baseUrl}}|");
    expect(accept('{"id": {{us|er}}}', (o) => o.label === "userId")).toBe('{"id": {{userId}}|}');
  });
});
