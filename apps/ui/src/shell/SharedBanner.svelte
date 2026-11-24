<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- A shared request (wave 17, strand C). A link that reached the app waits here: name, method
     and URL, and one button that opens it as an unsaved draft. Nothing runs and nothing is sent
     by opening or by waiting. After it was opened, the banner says which variables the request
     still needs. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";
  import { clearSharedInbox, sharedInbox } from "../share/inbox.svelte";
  import { Banner, Button } from "../ui";

  const app = getApp();
  let busy = $state(false);

  async function open() {
    const offer = sharedInbox.offer;
    if (!offer || busy) return;
    busy = true;
    try {
      await app.openShared(offer.input);
    } catch (e) {
      sharedInbox.error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if sharedInbox.offer}
  {@const p = sharedInbox.offer.payload}
  <div class="host" data-testid="shared-banner">
    <Banner tone="info" title="A request was shared with you" dismissible dismissLabel="Dismiss shared request" ondismiss={clearSharedInbox}>
      <span data-testid="shared-banner-summary"><strong>{p.name}</strong> · {p.request.method} {p.request.url}</span>
      <span class="muted">Opening it adds an unsaved draft. Nothing is sent.</span>
      {#snippet actions()}
        <Button variant="primary" size="sm" disabled={busy} data-testid="shared-open" onclick={open}>Open shared request</Button>
      {/snippet}
    </Banner>
  </div>
{:else if sharedInbox.opened}
  {@const o = sharedInbox.opened}
  <div class="host" data-testid="shared-opened">
    <Banner tone="ok" title="Shared request opened as a draft" dismissible dismissLabel="Dismiss" ondismiss={clearSharedInbox}>
      <strong>{o.name}</strong> is not saved and was not sent.
      {#if o.needs.length > 0}
        <span data-testid="shared-needs">It uses {o.needs.join(", ")}{o.secrets.length > 0 ? `; ${o.secrets.join(", ")} must be secret variables` : ""}. Define them in an environment.</span>
      {/if}
    </Banner>
  </div>
{:else if sharedInbox.error}
  <div class="host" data-testid="shared-error">
    <Banner tone="warn" title="This link could not be opened" dismissible dismissLabel="Dismiss" ondismiss={clearSharedInbox}>
      {sharedInbox.error}
    </Banner>
  </div>
{/if}

<style>
  .host {
    padding: var(--rt-space-2) var(--rt-space-4) 0;
  }
  .muted {
    display: block;
    color: var(--rt-text-muted, inherit);
  }
</style>
