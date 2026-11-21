<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Active environment picker. Owner: wave 3, strand D. Placeholder. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";

  const app = getApp();
</script>

<select
  data-testid="env-switcher"
  aria-label="Environment"
  value={app.environment ?? ""}
  onchange={(e) => app.setEnvironment(e.currentTarget.value || null)}
>
  <option value="">No environment</option>
  {#each app.environments as env (env.name)}
    <option value={env.name}>{env.name}</option>
  {/each}
</select>
