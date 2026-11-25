<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The "Shrink" action of a failed response: opens the minimal-case view for its request.
     Shown only when the response failed (status from 400, a failed test or assertion). -->
<script lang="ts">
  import type { StoredResponse } from "../commands/types";
  import type { AppState } from "../state/app.svelte";
  import { Button } from "../ui";
  import { failed } from "./model";
  import { openShrink } from "./open.svelte";

  let { app, path, result }: { app: AppState; path: string; result: StoredResponse | undefined } = $props();
</script>

{#if failed(result)}
  <Button size="sm" variant="ghost" icon="filter" data-testid="shrink-action" onclick={() => openShrink(app, path)}>Shrink</Button>
{/if}
