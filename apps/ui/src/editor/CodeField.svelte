<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- A CodeMirror 6 field with {{variable}} highlighting: single line (URL, table cells, auth
     fields) or multi line (body). Controlled: `value` in, `onchange` out. -->
<script lang="ts">
  import { Annotation, Compartment, EditorState } from "@codemirror/state";
  import { EditorView, placeholder as cmPlaceholder } from "@codemirror/view";
  import { onMount } from "svelte";

  import { base, language, multiLine, setLookup, singleLine, variables, type Language } from "./codemirror";
  import type { VarLookup } from "./vars";

  let {
    value,
    onchange,
    lookup,
    label,
    testid,
    multiline = false,
    lang = "text",
    placeholder = "",
    readonly = false,
    onenter,
  }: {
    value: string;
    onchange?: (value: string) => void;
    lookup: VarLookup;
    label: string;
    testid?: string;
    multiline?: boolean;
    lang?: Language;
    placeholder?: string;
    readonly?: boolean;
    onenter?: () => void;
  } = $props();

  /** Marks changes that come from `value`, so they are not reported back. */
  const external = Annotation.define<boolean>();
  const langSlot = new Compartment();
  const editSlot = new Compartment();
  let host: HTMLDivElement;
  let view: EditorView | undefined;

  const editable = (ro: boolean) => [EditorState.readOnly.of(ro), EditorView.editable.of(!ro)];

  onMount(() => {
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: value,
        extensions: [
          base(),
          variables(),
          multiline ? multiLine() : singleLine(() => onenter?.()),
          langSlot.of(language(lang)),
          editSlot.of(editable(readonly)),
          cmPlaceholder(placeholder),
          EditorView.contentAttributes.of({ "aria-label": label }),
          EditorView.updateListener.of((u) => {
            if (!u.docChanged || u.transactions.some((tr) => tr.annotation(external))) return;
            onchange?.(u.state.doc.toString());
          }),
        ],
      }),
    });
    view.dispatch({ effects: setLookup.of(lookup) });
    return () => view?.destroy();
  });

  $effect(() => {
    const v = value;
    if (!view) return;
    const current = view.state.doc.toString();
    if (v === current) return;
    // Keep the cursor where it was as far as possible.
    const head = Math.min(view.state.selection.main.head, v.length);
    view.dispatch({
      changes: { from: 0, to: current.length, insert: v },
      selection: { anchor: head },
      annotations: external.of(true),
    });
  });

  $effect(() => {
    const l = lookup;
    view?.dispatch({ effects: setLookup.of(l) });
  });

  $effect(() => {
    const l = lang;
    view?.dispatch({ effects: langSlot.reconfigure(language(l)) });
  });

  $effect(() => {
    const ro = readonly;
    view?.dispatch({ effects: editSlot.reconfigure(editable(ro)) });
  });
</script>

<div class="field" class:multiline data-testid={testid} bind:this={host}></div>

<style>
  .field {
    min-width: 0;
    flex: 1;
  }
  .field :global(.cm-editor) {
    height: 100%;
  }
  .multiline :global(.cm-editor) {
    min-height: 12em;
  }
  .field:not(.multiline) :global(.cm-scroller) {
    overflow-x: auto;
    scrollbar-width: none;
  }
</style>
