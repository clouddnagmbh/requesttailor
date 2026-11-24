<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Paste link" (wave 17, strand C): a shared link, a fragment (#rt=…) or an `rt:` payload
     opens as an unsaved draft. Nothing is sent. A cURL command pasted here is taken as well. -->
<script lang="ts">
  import type { ExtensionDialogProps } from "../extensions";
  import { getApp } from "../state/app.svelte";
  import { Button, DialogFooter, TextArea } from "../ui";
  import { classifyPaste } from "./detect";
  import { askCurl } from "../editor/paste-curl";

  let { close }: ExtensionDialogProps = $props();
  const app = getApp();
  let text = $state("");
  let error = $state<string | null>(null);
  let busy = $state(false);
  const kind = $derived(classifyPaste(text));

  async function submit() {
    if (busy || !kind) return;
    busy = true;
    error = null;
    try {
      if (kind === "share") await app.openShared(text);
      else await askCurl(app, text);
      close(true);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="paste">
  <p class="muted">Paste a RequestTailor link or a cURL command. It opens as an unsaved draft; nothing is sent.</p>
  <TextArea
    label="Link or command"
    rows={5}
    bind:value={text}
    data-testid="paste-link-input"
    placeholder="https://…#rt=1.… or rt:1.… or curl …"
    onkeydown={(e: KeyboardEvent) => {
      if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void submit();
    }}
  />
  {#if text.trim() && !kind}<p class="hint" data-testid="paste-link-hint">This is not a RequestTailor link or a cURL command.</p>{/if}
  {#if error}<p class="error" role="alert" data-testid="paste-link-error">{error}</p>{/if}
  <DialogFooter>
    <Button onclick={() => close(false)}>Cancel</Button>
    <Button variant="primary" disabled={!kind || busy} data-testid="paste-link-open" onclick={submit}>
      {kind === "curl" ? "Turn it into a request" : "Open shared request"}
    </Button>
  </DialogFooter>
</div>

<style>
  .paste {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
  }
  .muted,
  .hint {
    margin: 0;
    color: var(--rt-text-muted, inherit);
  }
  .error {
    margin: 0;
    color: var(--rt-danger-fg);
  }
</style>
