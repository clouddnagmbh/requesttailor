<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The dialog of an extension (`AppState.showDialog`, wave 14, strand C) in the shared Dialog
     frame (ui/Dialog.svelte): title bar, close button, Escape, focus kept inside, light and dark.
     The component can end its content with `DialogFooter` for its buttons. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";
  import Dialog from "@tailor-kit/ui/Dialog.svelte";
  import { getExtensions } from "./extension-tabs";

  const app = getApp();
  const extensions = getExtensions();
  const open = $derived(app.dialog);

  // Escape closes the extension's dialog before anything behind it sees the key.
  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && app.dialog) {
      e.preventDefault();
      e.stopPropagation();
      app.closeDialog();
    }
  }
</script>

<svelte:window onkeydowncapture={onkeydown} />

{#if open && extensions}
  {#key open.seq}
    <Dialog title={open.dialog.title} size={open.dialog.width ?? "md"} testid="extension-dialog" closeOnBackdrop={false} onclose={() => app.closeDialog()}>
      <open.dialog.component ctx={extensions.ctx} close={(r?: unknown) => app.closeDialog(r)} props={open.dialog.props ?? {}} />
    </Dialog>
  {/key}
{/if}
