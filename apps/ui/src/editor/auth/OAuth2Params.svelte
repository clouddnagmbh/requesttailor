<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Extra parameters of an OAuth 2.0 flow: which request (authorization, token, refresh),
     name, value and placement. The list stays grouped by step (`./oauth2.ts`). Wave 5,
     strand C. -->
<script lang="ts">
  import type { OAuth2Param, OAuth2Step, ParamPlacement } from "../../commands/types";
  import CodeField from "../CodeField.svelte";
  import type { VarLookup } from "../vars";
  import { STEPS, addParam, removeParam, updateParam } from "./oauth2";

  let {
    params,
    steps,
    lookup,
    onchange,
  }: {
    params: OAuth2Param[];
    /** Steps the flow has. */
    steps: OAuth2Step[];
    lookup: VarLookup;
    onchange: (params: OAuth2Param[], typing: boolean) => void;
  } = $props();

  const PLACEMENTS: [ParamPlacement, string][] = [
    ["query", "Query"],
    ["header", "Header"],
    ["body", "Body"],
  ];
  const stepLabel = (s: OAuth2Step) => STEPS.find(([v]) => v === s)?.[1] ?? s;
</script>

<div class="params" data-testid="oauth2-params">
  {#if params.length > 0}
    <table>
      <thead>
        <tr><th>Request</th><th>Name</th><th>Value</th><th>Send in</th><th><span class="sr">Remove</span></th></tr>
      </thead>
      <tbody>
        {#each params as param, i (i)}
          <tr data-testid="oauth2-param">
            <td>
              <select
                data-testid="oauth2-param-step"
                aria-label="Request of parameter {i + 1}"
                value={param.step}
                onchange={(e) => onchange(updateParam(params, i, { step: e.currentTarget.value as OAuth2Step }), false)}
              >
                {#each STEPS as [value] (value)}
                  {#if steps.includes(value) || value === param.step}
                    <option {value}>{stepLabel(value)}</option>
                  {/if}
                {/each}
              </select>
            </td>
            <td>
              <CodeField
                testid="oauth2-param-name"
                label="Name of parameter {i + 1}"
                placeholder="name"
                value={param.name}
                {lookup}
                onchange={(name) => onchange(updateParam(params, i, { name }), true)}
              />
            </td>
            <td>
              <CodeField
                testid="oauth2-param-value"
                label="Value of parameter {i + 1}"
                placeholder="value"
                value={param.value}
                {lookup}
                onchange={(value) => onchange(updateParam(params, i, { value }), true)}
              />
            </td>
            <td>
              <select
                data-testid="oauth2-param-placement"
                aria-label="Placement of parameter {i + 1}"
                value={param.placement}
                onchange={(e) => onchange(updateParam(params, i, { placement: e.currentTarget.value as ParamPlacement }), false)}
              >
                {#each PLACEMENTS as [value, text] (value)}
                  <option {value}>{text}</option>
                {/each}
              </select>
            </td>
            <td>
              <button data-testid="oauth2-param-remove" aria-label="Remove parameter {i + 1}" onclick={() => onchange(removeParam(params, i), false)}
                >×</button
              >
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
  <div class="add">
    {#each steps as step (step)}
      <button data-testid="oauth2-param-add-{step}" onclick={() => onchange(addParam(params, step), false)}>+ {stepLabel(step)} parameter</button>
    {/each}
  </div>
</div>

<style>
  .params {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th {
    text-align: left;
    color: var(--rt-fg-muted);
    font-weight: normal;
    padding: 2px 4px;
  }
  td {
    padding: 2px 4px;
    vertical-align: middle;
  }
  td:nth-child(2),
  td:nth-child(3) {
    width: 40%;
  }
  .add {
    display: flex;
    gap: var(--rt-gap);
    flex-wrap: wrap;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }
  select,
  button {
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px 6px;
  }
  button {
    cursor: pointer;
    background: var(--rt-bg-panel);
  }
  button:hover {
    background: var(--rt-bg-hover);
  }
</style>
