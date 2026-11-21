<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Active environment picker. Owner: wave 3, strand D. Production-looking names (prod, prd,
     live) are shown in the warning colour; that is visual only and changes no behaviour. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";
  import { isProductionName } from "./ops";

  const app = getApp();
  const prod = $derived(isProductionName(app.environment));
</script>

<span class="switcher" class:prod class:none={!app.environment}>
  <span class="dot" aria-hidden="true"></span>
  <select
    data-testid="env-switcher"
    data-prod={prod ? "true" : "false"}
    aria-label="Environment"
    title={prod ? "Production environment" : "Active environment"}
    value={app.environment ?? ""}
    onchange={(e) => app.setEnvironment(e.currentTarget.value || null)}
  >
    <option value="">No environment</option>
    {#each app.environments as env (env.name)}
      <option value={env.name}>{env.name}</option>
    {/each}
  </select>
  <span class="visually-hidden" data-testid="env-active" aria-live="polite">{app.environment ?? ""}</span>
</span>

<style>
  .switcher {
    display: inline-flex;
    align-items: center;
    gap: calc(var(--rt-gap) / 2);
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--rt-ok);
  }

  select {
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px var(--rt-gap);
  }

  .prod .dot {
    background: var(--rt-warn);
  }

  .prod select {
    border-color: var(--rt-warn);
    color: var(--rt-warn);
    font-weight: 600;
  }

  .none .dot {
    background: var(--rt-fg-muted);
  }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
