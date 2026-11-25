<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Allow <extension> to contact <host> for <purpose>?" (wave 19, strand B). Asked once per host
     and device. Says plainly what happens: one host, GET or POST, nothing else; names (never
     values) of the secrets that would be sent; the list of allowed hosts can take it back. -->
<script lang="ts">
  import type { ExtensionDialogProps } from "../extensions";
  import { Button, DialogFooter } from "../ui";
  import { hostLabel } from "./ask";

  let { close, props }: ExtensionDialogProps = $props();
  // svelte-ignore state_referenced_locally
  const { extension, purpose, host, port, method, secrets } = props as {
    extension: string;
    purpose: string;
    host: string;
    port: number | null;
    method: string;
    secrets: string[];
  };
</script>

<div class="body" data-testid="egress-ask">
  <p class="question">
    Allow <strong data-testid="egress-ask-extension">{extension}</strong> to contact
    <strong data-testid="egress-ask-host">{hostLabel(host, port)}</strong> for
    <strong data-testid="egress-ask-purpose">{purpose}</strong>?
  </p>
  <ul>
    <li>Only this host{port === null ? "" : " and port"}, and only GET or POST calls (this one is {method}).</li>
    <li>The call goes through the app’s send guard and is not recorded in the history.</li>
    {#if secrets.length > 0}
      <li data-testid="egress-ask-secrets">
        It sends the secret{secrets.length === 1 ? "" : "s"}
        {secrets.join(", ")} to this host. The value never shows in the answer.
      </li>
    {/if}
    <li>Nothing goes to RequestTailor servers. You can remove the host later under “Egress hosts”.</li>
  </ul>
</div>

<DialogFooter>
  <Button data-testid="egress-deny" onclick={() => close(false)}>Don’t allow</Button>
  <Button variant="primary" data-testid="egress-allow" onclick={() => close(true)}>Allow</Button>
</DialogFooter>

<style>
  .body {
    display: grid;
    gap: var(--rt-space-4);
  }
  .question {
    margin: 0;
    font-size: var(--rt-text-md, 1rem);
    overflow-wrap: anywhere;
  }
  ul {
    margin: 0;
    padding-left: var(--rt-space-6);
    display: grid;
    gap: var(--rt-space-2);
    color: var(--rt-text-muted, inherit);
    font-size: var(--rt-text-sm, 0.875rem);
  }
</style>
