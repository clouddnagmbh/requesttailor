<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- A CodeMirror 6 field with {{variable}} highlighting: single line (URL, table cells, auth
     fields) or multi line (body). Controlled: `value` in, `onchange` out.
     Wave 14: typing `{{` completes variables from the nearest `provideVarSource` (request
     editor, environment manager); `complete={false}` turns it off. Tooltips live at the end of
     the page (or of the open dialog), so no overflow of a narrow field ever clips them. -->
<script lang="ts">
  import { isolateHistory } from "@codemirror/commands";
  import { Annotation, Compartment, EditorState, Prec } from "@codemirror/state";
  import { closeCompletion } from "@codemirror/autocomplete";
  import { EditorView, keymap, placeholder as cmPlaceholder, tooltips } from "@codemirror/view";
  import { onMount } from "svelte";

  import EditorShell, { type EditorKind } from "../suggest/EditorShell.svelte";

  import { base, language, multiLine, setLookup, singleLine, variables, type Language } from "./codemirror";
  import { varCompletion } from "./var-completion";
  import { getVarSource } from "./var-source";
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
    onpastetext,
    onescape,
    ontake,
    complete = true,
    editor,
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
    /** Wave 17: the plain text of a paste, before it is inserted (the paste itself goes on). */
    onpastetext?: (text: string) => void;
    /** Wave 17: Escape; return `true` when it was used (otherwise it is not handled here). */
    onescape?: () => boolean;
    /** Wave 17: Enter while a paste hint waits; return `true` when it took the paste. */
    ontake?: () => boolean;
    /** `{{` completion of variables (needs a `provideVarSource` above). */
    complete?: boolean;
    /** Wave 16: which editor this is, so extensions can add actions and propose text (`EditorShell`). */
    editor?: EditorKind;
  } = $props();

  const varSource = getVarSource();

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
          tooltips({ parent: (host.closest("dialog") as HTMLElement | null) ?? document.body }),
          // svelte-ignore state_referenced_locally
          varSource && complete ? varCompletion(varSource) : [],
          multiline ? multiLine() : singleLine(() => onenter?.()),
          EditorView.domEventHandlers({
            paste: (event) => {
              const text = event.clipboardData?.getData("text/plain");
              if (text) {
                onpastetext?.(text);
                // A completion list open at the paste would take Enter; the hint needs it.
                queueMicrotask(() => view && closeCompletion(view));
                setTimeout(() => view && closeCompletion(view), 0);
              }
              return false;
            },
          }),
          Prec.high(
            keymap.of([
              { key: "Escape", run: () => onescape?.() ?? false },
              { key: "Enter", run: () => ontake?.() ?? false },
            ]),
          ),
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

  /** Wave 16: the whole text replaced as one undo step (an applied suggestion). */
  function replaceAll(next: string) {
    if (!view) return;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: next },
      annotations: isolateHistory.of("full"),
      userEvent: "input.suggestion",
    });
  }

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
      // Its own undo step (e.g. "Format"), never merged with typing around it.
      annotations: [external.of(true), isolateHistory.of("full")],
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

{#snippet fieldEl()}
  <div class="field" class:multiline data-testid={testid} bind:this={host}></div>
{/snippet}
{#if editor}
  <EditorShell
    {editor}
    text={() => view?.state.doc.toString() ?? value}
    selection={() => {
      const r = view?.state.selection.main;
      return r ? { from: r.from, to: r.to } : null;
    }}
    replace={replaceAll}
    focus={() => view?.focus()}
  >
    {@render fieldEl()}
  </EditorShell>
{:else}
  {@render fieldEl()}
{/if}

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
