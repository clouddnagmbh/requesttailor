<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Read-only, highlighted and foldable text (the response's pretty view and JSONPath
     results). The text is fixed for the component's lifetime; wrap it in {#key} to replace it. -->
<script lang="ts">
  import { EditorState } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import { onMount } from "svelte";

  import type { BodyLanguage } from "../pretty/language";
  import { readOnlyView } from "./codeview";

  let { text, lang, label, testid }: { text: string; lang: BodyLanguage; label: string; testid?: string } = $props();

  let host: HTMLDivElement;

  onMount(() => {
    const view = new EditorView({ parent: host, state: EditorState.create({ doc: text, extensions: readOnlyView(lang, label) }) });
    return () => view.destroy();
  });
</script>

<div class="code" data-testid={testid} data-language={lang} bind:this={host}></div>

<style>
  .code {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
  }
  .code :global(.cm-editor) {
    position: absolute;
    inset: 0;
  }
</style>
