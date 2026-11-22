// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Extensions of the read-only CodeMirror view behind the response's pretty view: highlighting
// by body language, line numbers, fold markers and the request editor's theme. Read-only, but
// focusable, so the cursor keys, selection and the fold keymap work from the keyboard.

import { defaultKeymap } from "@codemirror/commands";
import { html } from "@codemirror/lang-html";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { xml } from "@codemirror/lang-xml";
import { syntaxHighlighting } from "@codemirror/language";
import { EditorState, type Extension } from "@codemirror/state";
import { drawSelection, EditorView, highlightActiveLine, keymap, lineNumbers } from "@codemirror/view";

import { folding, theme, tokenHighlight } from "../editor/codemirror";
import type { BodyLanguage } from "../pretty/language";

/** CSS has no parser among the dependencies: shown with line numbers, without colours. */
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
    case "text":
      return [];
  }
}

const viewTheme = EditorView.theme({
  "&": { height: "100%", border: "none", borderRadius: "0", fontSize: "var(--rt-size-small)" },
  "&.cm-focused": { outline: "none" },
  "&.cm-focused .cm-content": { boxShadow: "none" },
  ".cm-scroller": { lineHeight: "18px", overscrollBehavior: "contain" },
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
    keymap.of(defaultKeymap),
    syntaxHighlighting(tokenHighlight),
    bodyLanguageSupport(lang),
    EditorView.contentAttributes.of({ "aria-label": label, "aria-readonly": "true" }),
  ];
}
