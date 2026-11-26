<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The dialog of an extension (`AppState.showDialog`, wave 14, strand C) in the shared dialog
     host (@tailor-kit/shell): title bar, close button, Escape, focus kept inside, light and dark.
     The component can end its content with `DialogFooter` for its buttons. -->
<script lang="ts">
  import { DialogHost } from "@tailor-kit/shell";

  import { getApp } from "../state/app.svelte";
  import { getExtensions } from "./extension-tabs";

  const app = getApp();
  const extensions = getExtensions();
  const hosted = $derived(app.dialog ? { ...app.dialog, title: app.dialog.dialog.title, width: app.dialog.dialog.width } : null);
</script>

{#if extensions}
  <DialogHost dialog={hosted} testid="extension-dialog" onclose={() => app.closeDialog()}>
    {#snippet content(open)}
      <open.dialog.component ctx={extensions.ctx} close={(r?: unknown) => app.closeDialog(r)} props={open.dialog.props ?? {}} />
    {/snippet}
  </DialogHost>
{/if}
