<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- A description draft (wave 16, strand B): read it, copy it, or save it as a Markdown file
     next to the request (shown as a diff first). The request model has no description field,
     so the draft is not written into the request itself. -->
<script lang="ts">
  import type { ExtensionDialogProps } from "../extensions";
  import { Button, DialogFooter } from "../ui";
  import { Markdown } from "@tailor-kit/ai-ui";

  let { close, props }: ExtensionDialogProps = $props();
  const docs = $derived(props.docs as string);
  let copied = $state(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(docs);
      copied = true;
    } catch {
      copied = false;
    }
  }
</script>

<div class="docs">
  <Markdown text={docs} testid="ai-docs-text" />
  <p class="muted">Saving writes <code>{props.file as string}</code>, unsaved until you save the collection.</p>
  <DialogFooter>
    <Button data-testid="ai-docs-copy" onclick={copy}>{copied ? "Copied" : "Copy"}</Button>
    <Button data-testid="ai-docs-discard" onclick={() => close()}>Discard</Button>
    <Button variant="primary" data-testid="ai-docs-save" onclick={() => close({ save: true })}>Review as file</Button>
  </DialogFooter>
</div>

<style>
  .docs {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
  }
  .muted {
    margin: 0;
    color: var(--rt-fg-muted);
    font: var(--rt-font-caption);
  }
</style>
