// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The `{{` completion of CodeFields (CodeMirror 6, @codemirror/autocomplete): typing `{{`
// opens the list of every variable the request sees at once (Ctrl+Space inside an open `{{…`
// too); typing filters it, prefix matches first. Each row shows the name, the value (`••••`
// for secrets, "from 1Password" for external ones), the layer and whether a higher layer
// overrides it; names used elsewhere but not defined come last, then "Create variable…",
// which opens a small form (`VarCreateForm.svelte`). Enter or Tab insert `name}}` (an
// existing `}}` is kept), Esc closes. The rules live in `var-complete.ts`.

import {
  acceptCompletion,
  autocompletion,
  closeCompletion,
  pickedCompletion,
  type Completion,
  type CompletionContext,
  type CompletionResult,
  type CompletionSection,
} from "@codemirror/autocomplete";
import { Prec, StateEffect, StateField, type EditorState, type Extension } from "@codemirror/state";
import { EditorView, keymap, showTooltip, type Tooltip } from "@codemirror/view";
import { mount, unmount } from "svelte";

import type { VariableScope } from "../commands/types";
import {
  completionEdit,
  entriesFrom,
  layerBadge,
  openPlaceholder,
  overrideText,
  rankEntries,
  validName,
  valueText,
  type CompletionEntry,
} from "./var-complete";
import VarCreateForm from "./VarCreateForm.svelte";
import type { VarSource } from "./var-source";

type Row = { kind: "entry"; entry: CompletionEntry; ranges: number[] } | { kind: "create"; name: string };

/** What each option of the list is, for rendering (options are created per query). */
const rows = new WeakMap<Completion, Row>();

function header(text: string): (section: CompletionSection) => HTMLElement {
  return () => {
    const el = document.createElement("completion-section");
    el.className = text ? "rt-vc-section" : "rt-vc-divider";
    el.textContent = text;
    return el;
  };
}

const UNDEFINED: CompletionSection = { name: "Not defined here", rank: 1, header: header("Not defined here") };
const CREATE: CompletionSection = { name: "create", rank: 2, header: header("") };

/** Replaces the open placeholder at the cursor with `name}}`. */
function insertName(view: EditorView, name: string, completion?: Completion): boolean {
  const head = view.state.selection.main.head;
  const line = view.state.doc.lineAt(head);
  const open = openPlaceholder(line.text, head - line.from);
  if (!open) return false;
  const e = completionEdit(line.text, open.start, head - line.from, name);
  view.dispatch({
    changes: { from: line.from + e.from, to: line.from + e.to, insert: e.insert },
    selection: { anchor: line.from + e.cursor },
    annotations: completion ? pickedCompletion.of(completion) : undefined,
    userEvent: "input.complete",
    scrollIntoView: true,
  });
  return true;
}

/** The options for `query` from `scope`. */
export function completionResult(scope: VariableScope, query: string, from: number, to: number): CompletionResult {
  const options: Completion[] = [];
  for (const { entry, match } of rankEntries(entriesFrom(scope), query)) {
    const option: Completion = {
      label: entry.name,
      type: "variable",
      section: entry.kind === "undefined" ? UNDEFINED : undefined,
      apply: (view, completion) => void insertName(view, entry.name, completion),
    };
    rows.set(option, { kind: "entry", entry, ranges: match.ranges });
    options.push(option);
  }
  const exists = scope.variables.some((v) => v.name === query);
  if (!exists && (query === "" || validName(query))) {
    const option: Completion = {
      label: query ? `Create variable “${query}”…` : "Create variable…",
      type: "create",
      section: CREATE,
      apply: (view) => {
        closeCompletion(view);
        view.dispatch({ effects: openCreate.of({ name: query, pos: view.state.selection.main.head }) });
      },
    };
    rows.set(option, { kind: "create", name: query });
    options.push(option);
  }
  return { from, to, options, filter: false, getMatch: (c) => (rows.get(c)?.kind === "entry" ? (rows.get(c) as { ranges: number[] }).ranges : []) };
}

/** The completion source (exported for tests): `null` outside an open `{{`, the cached scope
 *  synchronously, else after loading. */
export function varCompletionSource(vars: VarSource) {
  return (ctx: CompletionContext): CompletionResult | null | Promise<CompletionResult | null> => {
    const line = ctx.state.doc.lineAt(ctx.pos);
    const open = openPlaceholder(line.text, ctx.pos - line.from);
    if (!open) return null;
    const build = (scope: VariableScope) => completionResult(scope, open.query, line.from + open.start, ctx.pos);
    const cached = vars.cached();
    if (cached) return build(cached);
    return vars.load().then(build, () => null);
  };
}

// ---- rendering of a row ----

function span(className: string, text: string, title?: string): HTMLElement {
  const el = document.createElement("span");
  el.className = className;
  el.textContent = text;
  if (title) el.title = title;
  return el;
}

function renderDot(completion: Completion): Node | null {
  const row = rows.get(completion);
  if (!row) return null;
  if (row.kind === "create") return span("rt-vc-dot rt-vc-dot-create", "+");
  const e = row.entry;
  const state = e.kind === "undefined" ? "undefined" : e.secret ? "secret" : "plain";
  return span(`rt-vc-dot rt-vc-dot-${state}`, "");
}

function renderValue(completion: Completion): Node | null {
  const row = rows.get(completion);
  if (!row) return null;
  if (row.kind === "create") return span("rt-vc-value", "environment, collection or folder");
  const e = row.entry;
  const text = valueText(e);
  const kind = e.kind === "undefined" ? "undefined" : e.external ? "external" : e.secret ? "secret" : e.value === null || e.value === "" ? "empty" : "plain";
  const el = span(`rt-vc-value rt-vc-value-${kind}`, text);
  // The full value on hover; never a secret.
  if (kind === "plain" && e.value !== null && e.value !== text) el.title = e.value;
  return el;
}

function renderBadge(completion: Completion): Node | null {
  const row = rows.get(completion);
  if (!row || row.kind !== "entry" || !row.entry.source) return null;
  const wrap = document.createElement("span");
  wrap.className = "rt-vc-meta";
  const over = overrideText(row.entry);
  if (over) wrap.appendChild(span("rt-vc-over", "overridden", over));
  const b = layerBadge(row.entry.source);
  const badge = wrap.appendChild(span(`rt-vc-badge rt-vc-badge-${row.entry.source.layer}`, b.label, b.title));
  if (b.detail) badge.appendChild(span("rt-vc-badge-detail", b.detail));
  return wrap;
}

function optionClass(completion: Completion): string {
  const row = rows.get(completion);
  if (!row) return "";
  if (row.kind === "create") return "rt-vc-create";
  return [row.entry.overriddenBy ? "rt-vc-overridden" : "", row.entry.kind === "undefined" ? "rt-vc-undefined" : ""].join(" ").trim();
}

// ---- "Create variable" form ----

interface CreateState {
  name: string;
  pos: number;
}

const openCreate = StateEffect.define<CreateState>();
const closeCreate = StateEffect.define<null>();

function createField(vars: VarSource): StateField<CreateState | null> {
  return StateField.define<CreateState | null>({
    create: () => null,
    update(value, tr) {
      for (const e of tr.effects) {
        if (e.is(openCreate)) return e.value;
        if (e.is(closeCreate)) return null;
      }
      return value && tr.docChanged ? { ...value, pos: tr.changes.mapPos(value.pos) } : value;
    },
    provide: (field) =>
      showTooltip.compute([field], (state: EditorState): Tooltip | null => {
        const open = state.field(field);
        if (!open) return null;
        return {
          pos: open.pos,
          above: false,
          create: (view) => createTooltip(view, vars, open.name),
        };
      }),
  });
}

function createTooltip(view: EditorView, vars: VarSource, name: string) {
  const dom = document.createElement("div");
  dom.className = "rt-vc-create-tooltip";
  const close = (refocus: boolean) => {
    view.dispatch({ effects: closeCreate.of(null) });
    if (refocus) view.focus();
  };
  const component = mount(VarCreateForm, {
    target: dom,
    props: {
      name,
      targets: vars.targets(),
      oncancel: () => close(true),
      onsubmit: async (kind, variable) => {
        await vars.create(kind, variable);
        view.focus();
        insertName(view, variable.name);
        close(false);
      },
    },
  });
  return { dom, destroy: () => void unmount(component) };
}

// ---- look (tokens only) ----

const look = EditorView.theme({
  ".cm-tooltip.cm-tooltip-autocomplete.rt-vc": {
    padding: "4px",
    minWidth: "min(360px, calc(100vw - 24px))",
    maxWidth: "min(560px, calc(100vw - 24px))",
    fontFamily: "var(--rt-font-ui)",
    fontSize: "var(--rt-size)",
    color: "var(--rt-fg)",
    backgroundColor: "var(--rt-bg)",
    border: "1px solid var(--rt-border)",
    borderRadius: "8px",
    boxShadow: "var(--rt-shadow)",
  },
  ".cm-tooltip.cm-tooltip-autocomplete.rt-vc > ul": {
    fontFamily: "inherit",
    maxHeight: "min(320px, 45vh)",
    minWidth: "0",
    maxWidth: "none",
    scrollbarWidth: "thin",
    scrollPadding: "4px",
  },
  ".cm-tooltip.cm-tooltip-autocomplete.rt-vc > ul > li": {
    display: "flex",
    alignItems: "center",
    gap: "8px",
    padding: "5px 8px",
    borderRadius: "5px",
    lineHeight: "1.35",
    color: "var(--rt-fg)",
    cursor: "pointer",
  },
  ".cm-tooltip.cm-tooltip-autocomplete.rt-vc > ul > li:hover": { backgroundColor: "var(--rt-bg-hover)" },
  ".cm-tooltip.cm-tooltip-autocomplete.rt-vc > ul > li[aria-selected]": {
    color: "var(--rt-fg)",
    backgroundColor: "var(--rt-bg-selected)",
  },
  ".rt-vc .cm-completionLabel": {
    flex: "0 1 auto",
    minWidth: "0",
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
    fontFamily: "var(--rt-font-mono)",
    fontWeight: "500",
  },
  ".rt-vc .cm-completionMatchedText": { textDecoration: "none", color: "var(--rt-accent)", fontWeight: "700" },
  ".rt-vc-dot": {
    flex: "none",
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    width: "8px",
    height: "8px",
    borderRadius: "50%",
    backgroundColor: "var(--rt-var-resolved)",
  },
  ".rt-vc-dot-secret": { backgroundColor: "var(--rt-var-secret)" },
  ".rt-vc-dot-undefined": { backgroundColor: "transparent", boxShadow: "inset 0 0 0 1.5px var(--rt-var-unresolved)" },
  ".rt-vc-dot-create": {
    width: "16px",
    height: "16px",
    margin: "0 -4px",
    borderRadius: "4px",
    backgroundColor: "transparent",
    color: "var(--rt-accent)",
    fontWeight: "700",
  },
  ".rt-vc-value": {
    flex: "1 1 auto",
    minWidth: "0",
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
    fontFamily: "var(--rt-font-mono)",
    fontSize: "var(--rt-size-small)",
    color: "var(--rt-fg-muted)",
  },
  ".rt-vc-value-secret": { color: "var(--rt-var-secret)", letterSpacing: "1px" },
  ".rt-vc-value-external": { color: "var(--rt-var-secret)", fontFamily: "var(--rt-font-ui)" },
  ".rt-vc-value-empty, .rt-vc-value-undefined": { fontStyle: "italic", fontFamily: "var(--rt-font-ui)" },
  ".rt-vc-create .rt-vc-value": { fontFamily: "var(--rt-font-ui)" },
  ".rt-vc-create .cm-completionLabel": { fontFamily: "var(--rt-font-ui)", color: "var(--rt-accent)" },
  ".rt-vc-meta": { flex: "none", display: "inline-flex", alignItems: "center", gap: "6px", marginLeft: "auto" },
  ".rt-vc-badge": {
    display: "inline-flex",
    alignItems: "center",
    gap: "4px",
    maxWidth: "160px",
    padding: "0 6px",
    overflow: "hidden",
    whiteSpace: "nowrap",
    fontSize: "11px",
    lineHeight: "18px",
    color: "var(--rt-fg-muted)",
    backgroundColor: "var(--rt-bg-panel)",
    border: "1px solid var(--rt-border)",
    borderRadius: "999px",
  },
  ".rt-vc-badge-detail": { color: "var(--rt-fg)", fontWeight: "600", overflow: "hidden", textOverflow: "ellipsis" },
  ".rt-vc-over": { fontSize: "11px", color: "var(--rt-warn)" },
  ".rt-vc-overridden .cm-completionLabel, .rt-vc-overridden .rt-vc-value": {
    opacity: "0.6",
    textDecoration: "line-through",
    textDecorationColor: "var(--rt-fg-muted)",
  },
  ".rt-vc-undefined .cm-completionLabel": { color: "var(--rt-fg-muted)" },
  ".rt-vc completion-section.rt-vc-section": {
    display: "block",
    margin: "4px 0 0",
    padding: "6px 8px 2px",
    borderTop: "1px solid var(--rt-border)",
    borderBottom: "0",
    fontSize: "11px",
    fontWeight: "600",
    letterSpacing: "0.02em",
    textTransform: "uppercase",
    color: "var(--rt-fg-muted)",
    opacity: "1",
  },
  ".rt-vc completion-section.rt-vc-divider": {
    display: "block",
    height: "0",
    margin: "4px 0",
    padding: "0",
    border: "0",
    borderTop: "1px solid var(--rt-border)",
  },
  ".rt-vc-create-tooltip": {
    backgroundColor: "var(--rt-bg)",
    border: "1px solid var(--rt-border)",
    borderRadius: "8px",
    boxShadow: "var(--rt-shadow)",
  },
  ".cm-tooltip:has(> .rt-vc-create-tooltip)": { border: "0", backgroundColor: "transparent" },
});

/**
 * The completion for a CodeField: `vars` lists the variables and creates new ones. The scope
 * is (re)loaded when the field gets focus, so the list opens from memory.
 */
export function varCompletion(vars: VarSource): Extension {
  return [
    autocompletion({
      override: [varCompletionSource(vars)],
      activateOnTypingDelay: 0,
      icons: false,
      maxRenderedOptions: 200,
      tooltipClass: () => "rt-vc",
      optionClass,
      addToOptions: [
        { render: renderDot, position: 10 },
        { render: renderValue, position: 60 },
        { render: renderBadge, position: 70 },
      ],
    }),
    Prec.highest(keymap.of([{ key: "Tab", run: acceptCompletion }])),
    createField(vars),
    EditorView.domEventHandlers({
      focus: () => {
        vars.load().catch(() => undefined);
      },
    }),
    look,
  ];
}
