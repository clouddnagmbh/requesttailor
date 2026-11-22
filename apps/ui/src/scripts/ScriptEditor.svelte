<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- A JavaScript editor for one script (wave 7, strand C). CodeMirror 6; the JavaScript
     language is loaded when the first editor opens (the Scripts tab), so it costs nothing at
     start. Controlled: `value` in, `onchange` out. -->
<script lang="ts">
  import { Annotation, Compartment, EditorState } from "@codemirror/state";
  import { EditorView, placeholder as cmPlaceholder } from "@codemirror/view";
  import { onMount } from "svelte";

  import { base, multiLine } from "../editor/codemirror";

  let {
    value,
    onchange,
    label,
    testid,
    placeholder = "",
  }: {
    value: string;
    onchange: (value: string) => void;
    label: string;
    testid?: string;
    placeholder?: string;
  } = $props();

  const external = Annotation.define<boolean>();
  const langSlot = new Compartment();
  let host: HTMLDivElement;
  let view: EditorView | undefined;
  let language = $state<"loading" | "ready" | "failed">("loading");

  onMount(() => {
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: value,
        extensions: [
          base(),
          multiLine(),
          langSlot.of([]),
          cmPlaceholder(placeholder),
          EditorView.contentAttributes.of({ "aria-label": label, spellcheck: "false" }),
          EditorView.updateListener.of((u) => {
            if (!u.docChanged || u.transactions.some((tr) => tr.annotation(external))) return;
            onchange(u.state.doc.toString());
          }),
        ],
      }),
    });
    let alive = true;
    import("@codemirror/lang-javascript")
      .then((m) => {
        if (!alive || !view) return;
        view.dispatch({ effects: langSlot.reconfigure(m.javascript()) });
        language = "ready";
      })
      .catch(() => (language = "failed"));
    return () => {
      alive = false;
      view?.destroy();
    };
  });

  $effect(() => {
    const v = value;
    if (!view) return;
    const current = view.state.doc.toString();
    if (v === current) return;
    const head = Math.min(view.state.selection.main.head, v.length);
    view.dispatch({
      changes: { from: 0, to: current.length, insert: v },
      selection: { anchor: head },
      annotations: external.of(true),
    });
  });
</script>

<div class="field" data-testid={testid} data-language={language} bind:this={host}></div>

<style>
  .field {
    min-width: 0;
  }
  .field :global(.cm-editor) {
    min-height: 8em;
    max-height: 24em;
  }
  .field :global(.cm-scroller) {
    overflow: auto;
  }
</style>
