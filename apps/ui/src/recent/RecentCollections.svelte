<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Start screen: collections opened before on this device, one click to reopen
     (`backend.openRecent(id, true)`; in the browser the click may ask for the folder
     permission). The list loads after the shell's first frame (see ./startup.ts).
     Owner: wave 4, strand C. -->
<script lang="ts">
  import { onMount } from "svelte";

  import type { CollectionInfo, RecentCollection } from "../commands/backend";
  import { getApp } from "../state/app.svelte";
  import { afterFirstFrame } from "./startup";

  let { onopen }: { onopen: (info: CollectionInfo) => Promise<void> } = $props();
  const app = getApp();

  let recent = $state<RecentCollection[]>([]);
  let busy = $state<string | null>(null);
  let notice = $state<string | null>(null);

  async function refresh() {
    try {
      recent = await app.backend.recentCollections();
    } catch {
      recent = [];
    }
  }

  onMount(() => {
    let alive = true;
    void afterFirstFrame().then(async () => {
      if (alive) await refresh();
    });
    return () => {
      alive = false;
    };
  });

  async function open(r: RecentCollection) {
    if (busy) return;
    busy = r.id;
    notice = null;
    try {
      const info = await app.backend.openRecent(r.id, true);
      if (info) {
        await onopen(info);
      } else {
        notice = `“${r.name}” could not be opened: the folder is gone or access was not granted.`;
        await refresh();
      }
    } catch (e) {
      notice = e instanceof Error ? e.message : String(e);
    } finally {
      busy = null;
    }
  }
</script>

{#if recent.length > 0}
  <section class="recent" aria-labelledby="rt-recent-title" data-testid="recent-collections">
    <h2 id="rt-recent-title">Recent</h2>
    <ul>
      {#each recent as r (r.id)}
        <li>
          <button
            class="item"
            data-testid="recent-collection"
            title={r.location}
            disabled={busy !== null}
            onclick={() => open(r)}
          >
            <span class="name">{r.name}</span>
            <span class="location">{r.location}</span>
          </button>
        </li>
      {/each}
    </ul>
    {#if notice}<p class="notice" role="status" data-testid="recent-notice">{notice}</p>{/if}
  </section>
{/if}

<style>
  .recent {
    margin: 12px auto 20px;
    max-width: 420px;
    text-align: left;
  }
  h2 {
    margin: 0 0 6px;
    font-size: var(--rt-size-small);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--rt-fg-muted);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .item {
    display: flex;
    flex-direction: column;
    width: 100%;
    padding: 6px 10px;
    border: 1px solid transparent;
    border-radius: var(--rt-radius);
    background: none;
    color: var(--rt-fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .item:hover:not(:disabled) {
    background: var(--rt-bg-hover);
    border-color: var(--rt-border);
  }
  .item:disabled {
    cursor: default;
    opacity: 0.6;
  }
  .name {
    font-weight: 600;
  }
  .location {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .notice {
    margin: 6px 0 0;
    color: var(--rt-warn);
    font-size: var(--rt-size-small);
  }
</style>
