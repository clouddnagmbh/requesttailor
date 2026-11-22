// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// CodeMirror 6 extensions of the request editor: `{{variable}}` highlighting with provenance
// tooltips (hover and cursor), single-line fields, languages and a theme built only from the
// design tokens in tokens.css.

import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { json } from "@codemirror/lang-json";
import { xml } from "@codemirror/lang-xml";
import { bracketMatching, HighlightStyle, indentOnInput, syntaxHighlighting } from "@codemirror/language";
import { EditorState, RangeSetBuilder, StateEffect, StateField, type Extension } from "@codemirror/state";
import {
  Decoration,
  drawSelection,
  EditorView,
  highlightActiveLine,
  hoverTooltip,
  keymap,
  lineNumbers,
  showTooltip,
  ViewPlugin,
  type DecorationSet,
  type Tooltip,
  type ViewUpdate,
} from "@codemirror/view";
import { tags as t } from "@lezer/highlight";

import { describe, findPlaceholders, infoFor, type Placeholder, type VarLookup } from "./vars";

export type Language = "json" | "xml" | "text";

// ---- variable state ----

interface VarState {
  lookup: VarLookup;
  focused: boolean;
}

/** Replaces the lookup (after a new preview). */
export const setLookup = StateEffect.define<VarLookup>();
const setFocused = StateEffect.define<boolean>();

const varState = StateField.define<VarState>({
  create: () => ({ lookup: new Map(), focused: false }),
  update(value, tr) {
    for (const e of tr.effects) {
      if (e.is(setLookup)) value = { ...value, lookup: e.value };
      else if (e.is(setFocused)) value = { ...value, focused: e.value };
    }
    return value;
  },
});

function placeholderAt(state: EditorState, pos: number, strict: boolean): Placeholder | null {
  const line = state.doc.lineAt(pos);
  for (const p of findPlaceholders(line.text)) {
    const from = line.from + p.from;
    const to = line.from + p.to;
    if (strict ? from < pos && pos < to : from <= pos && pos <= to) return { ...p, from, to };
  }
  return null;
}

function tooltipDom(name: string, lookup: VarLookup): HTMLElement {
  const info = infoFor(lookup, name);
  const { title, detail } = describe(name, info);
  const dom = document.createElement("div");
  dom.className = `rt-var-tooltip rt-var-tooltip-${info.state}`;
  dom.dataset.testid = "var-tooltip";
  dom.setAttribute("role", "tooltip");
  const head = dom.appendChild(document.createElement("div"));
  head.className = "rt-var-tooltip-title";
  head.textContent = title;
  const body = dom.appendChild(document.createElement("div"));
  body.className = "rt-var-tooltip-detail";
  body.textContent = detail;
  return dom;
}

function makeTooltip(p: Placeholder, lookup: VarLookup): Tooltip {
  return { pos: p.from, end: p.to, above: true, create: () => ({ dom: tooltipDom(p.name, lookup) }) };
}

const hoverVars = hoverTooltip((view, pos) => {
  const p = placeholderAt(view.state, pos, false);
  return p ? makeTooltip(p, view.state.field(varState).lookup) : null;
});

/** Keyboard users get the same tooltip while the cursor is inside a placeholder. */
const cursorVars = showTooltip.compute([varState, "selection", "doc"], (state) => {
  const { focused, lookup } = state.field(varState);
  const sel = state.selection.main;
  if (!focused || !sel.empty) return null;
  const p = placeholderAt(state, sel.head, true);
  return p ? makeTooltip(p, lookup) : null;
});

function decorations(view: EditorView): DecorationSet {
  const { lookup } = view.state.field(varState);
  const builder = new RangeSetBuilder<Decoration>();
  let done = -1;
  for (const range of view.visibleRanges) {
    // Placeholders never span lines here; scan whole lines so none is cut at a range edge.
    const from = Math.max(view.state.doc.lineAt(range.from).from, done);
    const to = view.state.doc.lineAt(range.to).to;
    if (from >= to) continue;
    for (const p of findPlaceholders(view.state.sliceDoc(from, to))) {
      const state = infoFor(lookup, p.name).state;
      builder.add(
        from + p.from,
        from + p.to,
        Decoration.mark({
          class: `rt-var rt-var-${state}`,
          attributes: { "data-var": p.name, "data-var-state": state },
        }),
      );
    }
    done = to;
  }
  return builder.finish();
}

const highlightVars = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = decorations(view);
    }
    update(u: ViewUpdate) {
      const lookupChanged = u.transactions.some((tr) => tr.effects.some((e) => e.is(setLookup)));
      if (u.docChanged || u.viewportChanged || lookupChanged) this.decorations = decorations(u.view);
    }
  },
  { decorations: (v) => v.decorations },
);

export function variables(): Extension {
  return [
    varState,
    EditorView.focusChangeEffect.of((_, focusing) => setFocused.of(focusing)),
    highlightVars,
    hoverVars,
    cursorVars,
  ];
}

// ---- single line ----

/** One line: newlines from typing or pasting are dropped, Enter runs `onEnter`. */
export function singleLine(onEnter: () => void): Extension {
  return [
    EditorState.transactionFilter.of((tr) => {
      if (!tr.docChanged || tr.newDoc.lines <= 1) return tr;
      const text = tr.newDoc.toString().replace(/\r?\n/g, "");
      return [tr, { changes: { from: 0, to: tr.newDoc.length, insert: text }, selection: { anchor: text.length }, sequential: true }];
    }),
    keymap.of([
      {
        key: "Enter",
        run: () => {
          onEnter();
          return true;
        },
      },
    ]),
    EditorView.contentAttributes.of({ "aria-multiline": "false" }),
  ];
}

// ---- multi line ----

export function multiLine(): Extension {
  return [
    lineNumbers(),
    highlightActiveLine(),
    bracketMatching(),
    indentOnInput(),
    keymap.of([indentWithTab]),
    syntaxHighlighting(tokenHighlight),
  ];
}

export function language(lang: Language): Extension {
  switch (lang) {
    case "json":
      return json();
    case "xml":
      return xml();
    case "text":
      return [];
  }
}

export function base(): Extension {
  return [history(), drawSelection(), keymap.of([...defaultKeymap, ...historyKeymap]), theme];
}

// ---- look (tokens only) ----

const tokenHighlight = HighlightStyle.define([
  { tag: [t.propertyName, t.tagName], color: "var(--rt-accent)" },
  { tag: [t.string, t.attributeValue], color: "var(--rt-ok)" },
  { tag: [t.number, t.bool, t.null, t.attributeName], color: "var(--rt-warn)" },
  { tag: [t.comment, t.meta, t.processingInstruction], color: "var(--rt-fg-muted)" },
  { tag: [t.punctuation, t.bracket, t.angleBracket], color: "var(--rt-fg-muted)" },
  { tag: t.invalid, color: "var(--rt-danger)" },
  // Scripts (wave 7): JavaScript keywords.
  { tag: [t.keyword, t.controlKeyword, t.definitionKeyword, t.moduleKeyword, t.operatorKeyword], fontWeight: "600" },
]);

const theme = EditorView.theme({
  "&": {
    color: "var(--rt-fg)",
    backgroundColor: "var(--rt-bg)",
    border: "1px solid var(--rt-border)",
    borderRadius: "var(--rt-radius)",
    fontSize: "var(--rt-size)",
  },
  "&.cm-focused": { outline: "1px solid var(--rt-accent)", outlineOffset: "-1px" },
  ".cm-scroller": { fontFamily: "var(--rt-font-mono)", lineHeight: "1.5" },
  ".cm-content": { caretColor: "var(--rt-fg)", padding: "3px 0" },
  ".cm-line": { padding: "0 6px" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--rt-fg)" },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, ::selection":
    { backgroundColor: "var(--rt-bg-selected)" },
  ".cm-activeLine": { backgroundColor: "transparent" },
  "&.cm-focused .cm-activeLine": { backgroundColor: "var(--rt-bg-panel)" },
  ".cm-gutters": {
    backgroundColor: "var(--rt-bg-panel)",
    color: "var(--rt-fg-muted)",
    borderRight: "1px solid var(--rt-border)",
  },
  ".cm-activeLineGutter": { backgroundColor: "var(--rt-bg-hover)" },
  "&.cm-focused .cm-matchingBracket": { backgroundColor: "var(--rt-bg-hover)", outline: "1px solid var(--rt-border)" },
  ".cm-placeholder": { color: "var(--rt-fg-muted)" },
  ".rt-var": { borderRadius: "2px", fontWeight: "600" },
  ".rt-var-resolved": { color: "var(--rt-var-resolved)" },
  ".rt-var-secret": { color: "var(--rt-var-secret)" },
  ".rt-var-unresolved": { color: "var(--rt-var-unresolved)", textDecoration: "underline wavy var(--rt-var-unresolved)" },
  ".rt-var-unknown": { color: "var(--rt-fg-muted)", textDecoration: "underline dotted" },
  ".cm-tooltip": {
    backgroundColor: "var(--rt-bg-panel)",
    color: "var(--rt-fg)",
    border: "1px solid var(--rt-border)",
    borderRadius: "var(--rt-radius)",
  },
  ".rt-var-tooltip": { padding: "4px 8px", fontFamily: "var(--rt-font-ui)", fontSize: "var(--rt-size-small)", maxWidth: "40em" },
  ".rt-var-tooltip-title": { fontWeight: "600" },
  ".rt-var-tooltip-detail": { fontFamily: "var(--rt-font-mono)", wordBreak: "break-all", color: "var(--rt-fg-muted)" },
  ".rt-var-tooltip-secret .rt-var-tooltip-detail": { color: "var(--rt-var-secret)", fontStyle: "italic" },
  ".rt-var-tooltip-unresolved .rt-var-tooltip-title": { color: "var(--rt-var-unresolved)" },
});
