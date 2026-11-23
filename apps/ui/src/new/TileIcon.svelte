<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The icon of a tile in the "New…" dialog (wave 14, strand B): an icon of `./icons.ts` on a
     tinted square; unknown names get the generic tile icon. Placeholder until the shared icon set
     lands: the first letter of the name. -->
<script lang="ts">
  import { isNewIcon } from "./icons";

  let { name, kind }: { name: string; kind: string | null } = $props();
  const known = $derived(isNewIcon(name));
</script>

<span class="tile-icon" data-icon={known ? name : "generic"} data-kind={kind} aria-hidden="true">{known ? name.slice(0, 1).toUpperCase() : "+"}</span>

<style>
  .tile-icon {
    flex: none;
    display: inline-grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: var(--rt-radius);
    background: var(--rt-bg-panel);
    color: var(--rt-fg-muted);
    font-weight: 600;
  }
</style>
