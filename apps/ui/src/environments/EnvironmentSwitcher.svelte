<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Active environment picker. Owner: wave 3, strand D. Production environments (policy flag, else a name with prod, prd or live; wave 17) carry
     a PRODUCTION badge and the warning colour. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";
  import { getProduction } from "./production.svelte";

  const app = getApp();
  // Wave 17: the policy flag decides, the name only for environments it does not list.
  const production = getProduction(app);
  const prod = $derived(production.active);
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
  {#if prod}<span class="badge" data-testid="env-production-badge">PRODUCTION</span>{/if}
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

  .badge {
    padding: 0 var(--rt-space-3);
    border-radius: var(--rt-radius-pill);
    background: var(--rt-danger-solid);
    color: #fff;
    font-size: var(--rt-text-xs, 11px);
    font-weight: 700;
    letter-spacing: 0.05em;
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
