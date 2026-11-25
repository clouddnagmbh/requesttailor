<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Under the timing waterfall: the segments the send reported ("the server") and those an
     extension contributed (`ExtensionContext.segments`), one block per source. -->
<script lang="ts">
  import { onMount } from "svelte";

  import type { StoredResponse } from "../commands/types";
  import type { AppState } from "../state/app.svelte";
  import SegmentTree from "./SegmentTree.svelte";

  let { app, response, waitMs }: { app: AppState; response: StoredResponse; waitMs: number } = $props();

  let version = $state(0);
  onMount(() => app.segmentContributions.subscribe(() => version++));

  const reported = $derived(response.report.segments ?? []);
  const contributed = $derived.by(() => {
    void version;
    return response.response_id === null ? [] : app.segmentContributions.forResponse(response.response_id);
  });
</script>

{#if reported.length > 0}<SegmentTree source="server" segments={reported} {waitMs} />{/if}
{#each contributed as set (set.source)}
  <SegmentTree source={set.source} segments={set.segments} {waitMs} />
{/each}
