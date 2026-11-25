<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Egress hosts" (wave 19, strand B): the hosts the user allowed on this device for extensions,
     with who asked and why, and a button to take each one back. -->
<script lang="ts">
  import { onMount } from "svelte";

  import type { EgressHost } from "../commands/types";
  import type { ExtensionDialogProps } from "../extensions";
  import { Banner, Button, DialogFooter, EmptyState } from "../ui";
  import { hostLabel } from "./ask";

  let { ctx, close }: ExtensionDialogProps = $props();
  let hosts = $state<EgressHost[] | null>(null);
  let error = $state<string | null>(null);

  async function load() {
    try {
      hosts = await ctx.backend.egress.hosts();
      error = null;
    } catch (e) {
      hosts = [];
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function remove(h: EgressHost) {
    try {
      await ctx.backend.egress.remove(h.host, h.port ?? null);
      await load();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  const day = (ms: number) => new Date(ms).toISOString().slice(0, 10);

  onMount(() => void load());
</script>

<div class="body" data-testid="egress-hosts">
  <p class="lead">
    Extensions can only contact the hosts listed here, and only after you allowed them. The list is kept on this
    device.
  </p>
  {#if error}<Banner tone="danger" data-testid="egress-error">{error}</Banner>{/if}
  {#if hosts && hosts.length === 0}
    <EmptyState title="No hosts allowed" text="An extension asks the first time it needs one." size="sm" />
  {:else if hosts}
    <ul>
      {#each hosts as h (h.host + ":" + (h.port ?? ""))}
        <li data-testid="egress-host" data-host={hostLabel(h.host, h.port ?? null)}>
          <div class="what">
            <strong>{hostLabel(h.host, h.port ?? null)}</strong>
            <span class="why">{h.extension} · {h.purpose} · since {day(h.allowed_at)}</span>
          </div>
          <Button size="sm" variant="ghost" data-testid="egress-remove" onclick={() => remove(h)}>Remove</Button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<DialogFooter>
  <Button variant="primary" data-testid="egress-hosts-done" onclick={() => close()}>Done</Button>
</DialogFooter>

<style>
  .body {
    display: grid;
    gap: var(--rt-space-4);
  }
  .lead {
    margin: 0;
    color: var(--rt-text-muted, inherit);
    font-size: var(--rt-text-sm, 0.875rem);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: var(--rt-space-2);
  }
  li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--rt-space-4);
    padding: var(--rt-space-3) var(--rt-space-4);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
  }
  .what {
    display: grid;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .why {
    color: var(--rt-text-muted, inherit);
    font-size: var(--rt-text-sm, 0.875rem);
  }
</style>
