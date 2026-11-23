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
  completionStatus,
  pickedCompletion,
  type Completion,
  type CompletionContext,
  type CompletionResult,
  type CompletionSection,
} from "@codemirror/autocomplete";
import { Prec, StateEffect, StateField, type EditorState, type Extension } from "@codemirror/state";
import { EditorView, keymap, showTooltip, ViewPlugin, type Tooltip, type ViewUpdate } from "@codemirror/view";
import { mount, unmount, type Component } from "svelte";

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
import { Badge, Icon } from "../ui";
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
  return {
    from,
    to,
    options,
    filter: false,
    getMatch: (c) => (rows.get(c)?.kind === "entry" ? (rows.get(c) as { ranges: number[] }).ranges : []),
  };
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

// ---- rendering of a row (Icon and Badge from ../ui) ----

/** Components mounted into the rows of the open list; unmounted when it closes. */
const mounted = new Set<ReturnType<typeof mount>>();

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function mountInto(component: Component<any>, props: Record<string, unknown>): HTMLElement {
  const host = document.createElement("span");
  host.className = "rt-vc-slot";
  mounted.add(mount(component, { target: host, props }));
  return host;
}

const releaseRows = ViewPlugin.define(() => ({
  update(u: ViewUpdate) {
    if (mounted.size > 0 && completionStatus(u.state) === null && completionStatus(u.startState) !== null) {
      for (const c of mounted) void unmount(c);
      mounted.clear();
    }
  },
}));

function span(className: string, text: string, title?: string): HTMLElement {
  const el = document.createElement("span");
  el.className = className;
  el.textContent = text;
  if (title) el.title = title;
  return el;
}

function iconName(row: Row): string {
  if (row.kind === "create") return "add";
  const e = row.entry;
  if (e.kind === "undefined") return "warning";
  if (e.external) return "key";
  if (e.secret) return "lock";
  return "variable";
}

function renderIcon(completion: Completion): Node | null {
  const row = rows.get(completion);
  if (!row) return null;
  const el = mountInto(Icon, { name: iconName(row), size: "sm" });
  el.classList.add(`rt-vc-icon-${iconName(row)}`);
  return el;
}

function renderValue(completion: Completion): Node | null {
  const row = rows.get(completion);
  if (!row) return null;
  if (row.kind === "create") return span("rt-vc-value rt-vc-value-hint", "in the environment, collection or folder");
  const e = row.entry;
  const text = valueText(e);
  const kind =
    e.kind === "undefined"
      ? "undefined"
      : e.external
        ? "external"
        : e.secret
          ? "secret"
          : e.value === null || e.value === ""
            ? "empty"
            : "plain";
  const el = span(`rt-vc-value rt-vc-value-${kind}`, text);
  // The full value on hover; never a secret.
  if (kind === "plain" && e.value !== null && e.value !== text) el.title = e.value;
  return el;
}

const TONES: Record<string, "neutral" | "accent" | "info" | "ok" | "warn"> = {
  global: "neutral",
  collection: "neutral",
  environment: "info",
  folder: "neutral",
  request: "accent",
  runtime: "warn",
};

function renderBadge(completion: Completion): Node | null {
  const row = rows.get(completion);
  if (!row || row.kind !== "entry" || !row.entry.source) return null;
  const wrap = document.createElement("span");
  wrap.className = "rt-vc-meta";
  const over = overrideText(row.entry);
  if (over) wrap.appendChild(span("rt-vc-over", "overridden", over));
  const b = layerBadge(row.entry.source);
  const badge = mountInto(Badge, {
    value: b.detail ? `${b.label} · ${b.detail}` : b.label,
    tone: TONES[row.entry.source.layer] ?? "neutral",
    title: b.title,
  });
  badge.classList.add("rt-vc-badge");
  wrap.appendChild(badge);
  return wrap;
}

function optionClass(completion: Completion): string {
  const row = rows.get(completion);
  if (!row) return "";
  if (row.kind === "create") return "rt-vc-create";
  return [row.entry.overriddenBy ? "rt-vc-overridden" : "", row.entry.kind === "undefined" ? "rt-vc-undefined" : ""]
    .join(" ")
    .trim();
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
        return { pos: open.pos, above: false, create: (view) => createTooltip(view, vars, open.name) };
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

// ---- look: the Menu of ../ui (surface 3, elevation 2, rows of control height) ----

const POPUP = ".cm-tooltip.cm-tooltip-autocomplete.rt-vc";

const look = EditorView.theme({
  [POPUP]: {
    zIndex: "var(--rt-z-popover)",
    boxSizing: "border-box",
    padding: "var(--rt-space-2)",
    minWidth: "min(380px, calc(100vw - 24px))",
    maxWidth: "min(560px, calc(100vw - 24px))",
    font: "var(--rt-font-body)",
    color: "var(--rt-fg)",
    backgroundColor: "var(--rt-surface-3)",
    border: "1px solid var(--rt-border)",
    borderRadius: "var(--rt-radius-lg)",
    boxShadow: "var(--rt-elev-2)",
    animation: "rt-vc-in var(--rt-dur-fast) var(--rt-ease-out)",
  },
  "@keyframes rt-vc-in": { from: { opacity: "0" } },
  [`${POPUP} > ul`]: {
    font: "inherit",
    maxHeight: "min(320px, 45vh)",
    minWidth: "0",
    maxWidth: "none",
    scrollbarWidth: "thin",
    scrollbarColor: "var(--rt-scrollbar) transparent",
    scrollPaddingBlock: "var(--rt-space-2)",
  },
  [`${POPUP} > ul > li`]: {
    display: "flex",
    alignItems: "center",
    gap: "var(--rt-space-4)",
    boxSizing: "border-box",
    height: "var(--rt-control-md)",
    padding: "0 var(--rt-space-4)",
    borderRadius: "var(--rt-radius-md)",
    color: "var(--rt-fg)",
    cursor: "pointer",
  },
  [`${POPUP} > ul > li:hover`]: { backgroundColor: "var(--rt-bg-hover)" },
  [`${POPUP} > ul > li[aria-selected]`]: { color: "var(--rt-fg)", backgroundColor: "var(--rt-bg-active)" },
  ".rt-vc .cm-completionLabel": {
    // Values line up in a column for usual name lengths; long names still shrink.
    flex: "0 1 auto",
    width: "13ch",
    minWidth: "0",
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
    fontFamily: "var(--rt-font-mono)",
    fontSize: "var(--rt-text-small)",
    fontWeight: "var(--rt-weight-medium)",
  },
  ".rt-vc .cm-completionMatchedText": {
    textDecoration: "none",
    color: "var(--rt-accent)",
    fontWeight: "var(--rt-weight-bold)",
  },
  ".rt-vc-slot": { display: "inline-flex", flex: "none", alignItems: "center" },
  ".rt-vc-slot .rt-icon": { color: "var(--rt-fg-muted)" },
  ".rt-vc-icon-variable .rt-icon": { color: "var(--rt-var-resolved)" },
  ".rt-vc-icon-lock .rt-icon, .rt-vc-icon-key .rt-icon": { color: "var(--rt-var-secret)" },
  ".rt-vc-icon-warning .rt-icon": { color: "var(--rt-var-unresolved)" },
  ".rt-vc-icon-add .rt-icon": { color: "var(--rt-accent)" },
  ".rt-vc-value": {
    flex: "1 1 auto",
    minWidth: "0",
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
    fontFamily: "var(--rt-font-mono)",
    fontSize: "var(--rt-text-small)",
    color: "var(--rt-fg-muted)",
  },
  ".rt-vc-value-secret": { color: "var(--rt-var-secret)", letterSpacing: "0.1em" },
  ".rt-vc-value-external": { font: "var(--rt-font-small)", color: "var(--rt-var-secret)" },
  ".rt-vc-value-empty, .rt-vc-value-undefined, .rt-vc-value-hint": {
    font: "var(--rt-font-small)",
    color: "var(--rt-fg-subtle)",
  },
  ".rt-vc-create .cm-completionLabel": { font: "var(--rt-font-body)", color: "var(--rt-accent)" },
  ".rt-vc-meta": {
    flex: "none",
    display: "inline-flex",
    alignItems: "center",
    gap: "var(--rt-space-3)",
    marginLeft: "auto",
  },
  ".rt-vc-badge .rt-badge": { maxWidth: "160px", overflow: "hidden", textOverflow: "ellipsis" },
  ".rt-vc-over": { font: "var(--rt-font-small)", color: "var(--rt-warn-fg)" },
  ".rt-vc-overridden .cm-completionLabel, .rt-vc-overridden .rt-vc-value": {
    color: "var(--rt-fg-subtle)",
    textDecoration: "line-through",
  },
  ".rt-vc-undefined .cm-completionLabel": { color: "var(--rt-fg-muted)" },
  [`${POPUP} completion-section.rt-vc-section`]: {
    display: "block",
    margin: "var(--rt-space-2) 0 0",
    padding: "var(--rt-space-3) var(--rt-space-4) var(--rt-space-2)",
    border: "0",
    borderTop: "1px solid var(--rt-border)",
    font: "var(--rt-font-caption)",
    letterSpacing: "var(--rt-tracking-caps)",
    textTransform: "uppercase",
    color: "var(--rt-fg-subtle)",
    opacity: "1",
  },
  [`${POPUP} completion-section.rt-vc-divider`]: {
    display: "block",
    height: "1px",
    margin: "var(--rt-space-2) calc(-1 * var(--rt-space-2))",
    padding: "0",
    border: "0",
    backgroundColor: "var(--rt-border)",
  },
  ".cm-tooltip:has(> .rt-vc-create-tooltip)": {
    zIndex: "var(--rt-z-popover)",
    color: "var(--rt-fg)",
    backgroundColor: "var(--rt-surface-3)",
    border: "1px solid var(--rt-border)",
    borderRadius: "var(--rt-radius-lg)",
    boxShadow: "var(--rt-elev-2)",
    animation: "rt-vc-in var(--rt-dur-fast) var(--rt-ease-out)",
  },
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
      // The list opens the moment `{{` is typed, so a key right after it is meant for it.
      interactionDelay: 0,
      icons: false,
      maxRenderedOptions: 200,
      tooltipClass: () => "rt-vc",
      optionClass,
      addToOptions: [
        { render: renderIcon, position: 10 },
        { render: renderValue, position: 60 },
        { render: renderBadge, position: 70 },
      ],
    }),
    Prec.highest(keymap.of([{ key: "Tab", run: acceptCompletion }])),
    createField(vars),
    releaseRows,
    EditorView.domEventHandlers({
      focus: () => {
        vars.load().catch(() => undefined);
      },
    }),
    look,
  ];
}
