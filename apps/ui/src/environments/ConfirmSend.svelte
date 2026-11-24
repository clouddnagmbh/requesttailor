<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Send POST to PRD?": the question before a writing request goes to a production
     environment (wave 17, strand D), or before a run does. Enter confirms (the confirm button
     has the focus), Escape cancels. "Don't ask again on this device" switches it off here;
     reading requests are never asked. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";
  import Button from "../ui/Button.svelte";
  import Checkbox from "../ui/Checkbox.svelte";
  import Dialog from "../ui/Dialog.svelte";
  import DialogFooter from "../ui/DialogFooter.svelte";
  import { confirmQuestion, setConfirmEnabled } from "./production";
  import { getProduction } from "./production.svelte";

  const app = getApp();
  const production = getProduction(app);
  const pending = $derived(production.pending);
  let never = $state(false);

  function answer(yes: boolean): void {
    if (yes && never) setConfirmEnabled(false);
    never = false;
    production.answer(yes);
  }
</script>

{#if pending}
  <Dialog
    title={pending.kind === "run" ? `Run against ${pending.environment}?` : confirmQuestion(pending)}
    description="{pending.environment} is a production environment."
    icon="warning"
    size="sm"
    testid="confirm-send"
    closeOnBackdrop={false}
    onclose={() => answer(false)}
  >
    <dl class="summary" data-testid="confirm-send-summary">
      <dt>{pending.kind === "run" ? "Runs" : "Request"}</dt>
      <dd>{#if pending.kind === "send"}<strong>{pending.method}</strong> {/if}<code>{pending.url}</code></dd>
      <dt>Environment</dt>
      <dd><strong class="env">{pending.environment}</strong></dd>
    </dl>
    <Checkbox bind:checked={never} label="Don't ask again on this device" data-testid="confirm-send-never" />
    <DialogFooter>
      <Button variant="secondary" data-testid="confirm-send-cancel" onclick={() => answer(false)}>Cancel</Button>
      <Button variant="danger" data-testid="confirm-send-yes" data-autofocus onclick={() => answer(true)}
        >{pending.kind === "run" ? "Run" : "Send"}</Button
      >
    </DialogFooter>
  </Dialog>
{/if}

<style>
  .summary {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--rt-space-2) var(--rt-space-4);
    margin: 0 0 var(--rt-space-5);
  }
  dt {
    color: var(--rt-fg-muted);
  }
  dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .env {
    color: var(--rt-danger-fg);
  }
</style>
