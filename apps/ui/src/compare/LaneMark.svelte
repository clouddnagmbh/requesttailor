<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The lane rail of one revision in the picker (wave 14, strand H): the branch lanes as thin
     vertical lines, the revision's own as a dot on its lane, as in the commit graph. Lane 0 (a
     tag) is a diamond to the right of the lanes. -->
<script lang="ts">
  let { lane, lanes, kind = "branch" }: { lane: number; lanes: number; kind?: "branch" | "remote" | "tag" | "commit" } = $props();

  const STEP = 10;
  const width = $derived(Math.max(1, lanes) * STEP + (lane === 0 ? STEP : 0));
  const x = (n: number) => (n - 1) * STEP + STEP / 2;
</script>

<svg class="lanes" {width} height="24" viewBox="0 0 {width} 24" aria-hidden="true">
  {#each Array.from({ length: lanes }, (_, i) => i + 1) as n (n)}
    <line x1={x(n)} x2={x(n)} y1="0" y2="24" stroke="var(--rt-lane-{n})" stroke-opacity={n === lane ? 0.9 : 0.28} stroke-width="2" />
  {/each}
  {#if lane === 0}
    <rect x={lanes * STEP + 1} y="8" width="8" height="8" transform="rotate(45 {lanes * STEP + 5} 12)" fill="var(--rt-fg-muted)" />
  {:else if kind === "commit"}
    <circle cx={x(lane)} cy="12" r="3.5" fill="var(--rt-bg)" stroke="var(--rt-lane-{lane})" stroke-width="2" />
  {:else}
    <circle cx={x(lane)} cy="12" r="4.5" fill="var(--rt-lane-{lane})" stroke={kind === "remote" ? "var(--rt-bg)" : "none"} stroke-width="1.5" />
  {/if}
</svg>

<style>
  .lanes {
    flex: none;
    display: block;
  }
</style>
