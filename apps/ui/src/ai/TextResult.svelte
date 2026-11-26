<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- An answer that is only read (wave 16, strand B): the summary of a response. In the shell's
     dialog place (`AppState.showDialog`); nothing is changed by it. -->
<script lang="ts">
  import type { ExtensionDialogProps } from "../extensions";
  import { Button, DialogFooter } from "../ui";
  import { Markdown } from "@tailor-kit/ai-ui";

  let { close, props }: ExtensionDialogProps = $props();
</script>

<div class="result">
  <p class="muted" data-testid="ai-result-path">{props.path as string}</p>
  <Markdown text={props.text as string} testid="ai-result-text" />
  <DialogFooter>
    <Button variant="primary" data-testid="ai-result-close" onclick={() => close()}>Close</Button>
  </DialogFooter>
</div>

<style>
  .result {
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
