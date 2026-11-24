<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The hint under the URL field after a paste of a cURL command or a share link (wave 17,
     strand C). Enter takes it, Escape (or the button) leaves the text as it is. -->
<script lang="ts">
  import { hintText, type PasteOffer } from "../editor/paste-curl";
  import { Banner, Button, Kbd } from "../ui";

  let {
    offer,
    replaces,
    note,
    ontake,
    ondismiss,
  }: {
    offer: PasteOffer | null;
    /** The request has content that would be replaced. */
    replaces: boolean;
    /** What the importer left out, after a cURL command was taken. */
    note: string | null;
    ontake: () => void;
    ondismiss: () => void;
  } = $props();
</script>

{#if offer}
  <div class="hint" data-testid="paste-hint">
    <Banner tone="info" dismissible dismissLabel="Keep the text" ondismiss={ondismiss}>
      {hintText(offer, replaces)}
      <span class="keys"><Kbd keys="Enter" size="sm" /> to take it, <Kbd keys="Escape" size="sm" /> to keep the text.</span>
      {#snippet actions()}
        <Button variant="primary" size="sm" data-testid="paste-hint-take" onclick={ontake}>
          {offer.kind === "curl" ? "Turn it into a request" : "Open shared request"}
        </Button>
      {/snippet}
    </Banner>
  </div>
{:else if note}
  <div class="hint" data-testid="paste-note">
    <Banner tone="ok" dismissible dismissLabel="Dismiss" ondismiss={ondismiss}>
      <span style="white-space: pre-line">{note}</span>
    </Banner>
  </div>
{/if}

<style>
  .hint {
    margin-bottom: var(--rt-space-2);
  }
  .keys {
    margin-left: var(--rt-space-2);
  }
</style>
