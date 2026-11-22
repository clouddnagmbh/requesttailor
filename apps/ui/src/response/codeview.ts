// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Extensions of the read-only CodeMirror view behind the response's pretty view: highlighting
// by body language, line numbers, fold markers, search (Mod+F) and the request editor's theme.
// Read-only, but focusable, so the cursor keys, selection, search and the fold keymap work
// from the keyboard.

import { defaultKeymap } from "@codemirror/commands";
import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { xml } from "@codemirror/lang-xml";
import { syntaxHighlighting } from "@codemirror/language";
import { search, searchKeymap } from "@codemirror/search";
import { EditorState, type Extension } from "@codemirror/state";
import { drawSelection, EditorView, highlightActiveLine, keymap, lineNumbers } from "@codemirror/view";

import { folding, theme, tokenHighlight } from "../editor/codemirror";
import type { BodyLanguage } from "../pretty/language";

export function bodyLanguageSupport(lang: BodyLanguage): Extension {
  switch (lang) {
    case "json":
      return json();
    case "xml":
      return xml();
    case "html":
      return html();
    case "javascript":
      return javascript();
    case "css":
      return css();
    case "text":
      return [];
  }
}

const viewTheme = EditorView.theme({
  "&": { height: "100%", border: "none", borderRadius: "0", fontSize: "var(--rt-size-small)" },
  "&.cm-focused": { outline: "none" },
  "&.cm-focused .cm-content": { boxShadow: "none" },
  ".cm-scroller": { lineHeight: "18px", overscrollBehavior: "contain" },
  // Search panel (Mod+F): tokens only.
  ".cm-panels": { backgroundColor: "var(--rt-bg-panel)", color: "var(--rt-fg)" },
  ".cm-panels-top": { borderBottom: "1px solid var(--rt-border)" },
  ".cm-panel.cm-search": { fontFamily: "var(--rt-font-ui)", fontSize: "var(--rt-size-small)", padding: "4px 24px 4px 8px" },
  ".cm-panel.cm-search input, .cm-panel.cm-search button": {
    font: "inherit",
    color: "var(--rt-fg)",
    backgroundColor: "var(--rt-bg)",
    backgroundImage: "none",
    border: "1px solid var(--rt-border)",
    borderRadius: "var(--rt-radius)",
  },
  ".cm-panel.cm-search input[type=checkbox]": { verticalAlign: "middle" },
  ".cm-panel.cm-search button:hover": { backgroundColor: "var(--rt-bg-hover)" },
  ".cm-panel.cm-search [name=close]": { color: "var(--rt-fg-muted)", border: "none", backgroundColor: "transparent" },
  ".cm-searchMatch": { backgroundColor: "var(--rt-bg-hover)", outline: "1px solid var(--rt-border)" },
  ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "var(--rt-bg-selected)", outline: "1px solid var(--rt-accent)" },
});

export function readOnlyView(lang: BodyLanguage, label: string): Extension {
  return [
    theme,
    viewTheme,
    EditorState.readOnly.of(true),
    lineNumbers(),
    folding(),
    highlightActiveLine(),
    drawSelection(),
    // Mod+F opens the search panel; Enter / F3 / Mod+G next, with Shift previous; Escape closes.
    search({ top: true }),
    keymap.of([...searchKeymap, ...defaultKeymap]),
    syntaxHighlighting(tokenHighlight),
    bodyLanguageSupport(lang),
    EditorView.contentAttributes.of({ "aria-label": label, "aria-readonly": "true" }),
  ];
}
