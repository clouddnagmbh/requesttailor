<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The GraphQL query of a request (wave 8, strand B): CodeMirror 6 with GraphQL syntax,
     completion and validation from the schema (cm6-graphql), and {{variable}} highlighting
     like every other field. Controlled: `value` in, `onchange` out. -->
<script lang="ts">
  import { autocompletion } from "@codemirror/autocomplete";
  import { Annotation, EditorState } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import { graphql, updateSchema } from "cm6-graphql";
  import type { GraphQLSchema } from "graphql";
  import { onMount } from "svelte";

  import { base, multiLine, setLookup, variables } from "../editor/codemirror";
  import type { VarLookup } from "../editor/vars";

  let {
    value,
    onchange,
    lookup,
    schema,
  }: {
    value: string;
    onchange: (value: string) => void;
    lookup: VarLookup;
    /** `null` until loaded: no completion or validation then. */
    schema: GraphQLSchema | null;
  } = $props();

  const external = Annotation.define<boolean>();
  let host: HTMLDivElement;
  let view: EditorView | undefined;

  onMount(() => {
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: value,
        extensions: [
          base(),
          variables(),
          multiLine(),
          graphql(schema ?? undefined),
          autocompletion({ activateOnTyping: true }),
          EditorView.contentAttributes.of({ "aria-label": "GraphQL query" }),
          EditorView.updateListener.of((u) => {
            if (!u.docChanged || u.transactions.some((tr) => tr.annotation(external))) return;
            onchange(u.state.doc.toString());
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
    const head = Math.min(view.state.selection.main.head, v.length);
    view.dispatch({
      changes: { from: 0, to: current.length, insert: v },
      selection: { anchor: head },
      annotations: external.of(true),
    });
  });

  $effect(() => {
    const s = schema;
    if (view) updateSchema(view, s ?? undefined);
  });

  $effect(() => {
    const l = lookup;
    view?.dispatch({ effects: setLookup.of(l) });
  });
</script>

<div class="field" data-testid="graphql-query" bind:this={host}></div>

<style>
  .field {
    min-height: 160px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    overflow: hidden;
  }
  .field:focus-within {
    border-color: var(--rt-focus);
  }
</style>
