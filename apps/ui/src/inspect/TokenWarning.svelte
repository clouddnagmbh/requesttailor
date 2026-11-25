<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Token expired 12 min ago. Send anyway?" (wave 18, strand B): the dialog of the token gate.
     Enter sends (the send button has the focus), Escape cancels. With an OAuth auth block it also
     offers "Get a new token". The token's value is not shown here. -->
<script lang="ts">
  import type { ExtensionDialogProps } from "../extensions";
  import { Button, Checkbox, DialogFooter } from "../ui";
  import { setWarningEnabled } from "./settings";

  let { close, props }: ExtensionDialogProps = $props();
  // svelte-ignore state_referenced_locally
  const { text, where, oauth } = props as { text: string; where: string; oauth: boolean };
  let never = $state(false);

  function answer(value: "send" | "new-token") {
    if (never) setWarningEnabled(false);
    close(value);
  }
</script>

<div data-testid="token-warning">
  <p class="what" data-testid="token-warning-text">{text}</p>
  <p class="where">Found in the {where}. Send anyway?</p>
  <Checkbox bind:checked={never} label="Don't warn about tokens on this device" data-testid="token-warning-never" />
  <DialogFooter>
    <Button variant="secondary" data-testid="token-warning-cancel" onclick={() => close(undefined)}>Cancel</Button>
    {#if oauth}
      <Button variant="secondary" data-testid="token-warning-new" onclick={() => answer("new-token")}>Get a new token</Button>
    {/if}
    <Button variant="primary" data-testid="token-warning-send" data-autofocus onclick={() => answer("send")}>Send anyway</Button>
  </DialogFooter>
</div>

<style>
  .what {
    margin: 0 0 var(--rt-space-2);
    font-weight: 600;
  }
  .where {
    margin: 0 0 var(--rt-space-5);
    color: var(--rt-fg-muted);
  }
</style>
