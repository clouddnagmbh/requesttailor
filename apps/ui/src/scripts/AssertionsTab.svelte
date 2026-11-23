<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The request editor's "Assertions" tab (wave 7, strand C): declarative checks
     (`RequestSpec.assertions`, e.g. `res.status eq 200`) and "take a value from the response
     into a variable" (`RequestSpec.actions`, OpenCollection `set-variable`). Both run in the
     core without JavaScript, so they need no trust and behave the same everywhere. -->
<script lang="ts">
  import SuggestAssertionsButton from "../ai/SuggestAssertionsButton.svelte";
  import type { Assertion, VarAction } from "../commands/types";
  import { isUnary, METHODS, newAction, newAssertion, OPERATORS, SCOPES, valueHint } from "./edit";

  let {
    assertions,
    actions,
    onassertions,
    onactions,
  }: {
    assertions: Assertion[];
    actions: VarAction[];
    /** `typing`: debounce the edit. */
    onassertions: (next: Assertion[], typing: boolean) => void;
    onactions: (next: VarAction[], typing: boolean) => void;
  } = $props();

  function setAssertion(i: number, patch: Partial<Assertion>, typing = false) {
    onassertions(
      assertions.map((a, j) => (j === i ? { ...a, ...patch } : a)),
      typing,
    );
  }
  function setAction(i: number, patch: Partial<VarAction>, typing = false) {
    onactions(
      actions.map((a, j) => (j === i ? { ...a, ...patch } : a)),
      typing,
    );
  }
  const known = (op: string) => (OPERATORS as readonly string[]).includes(op);
</script>

<div class="assertions" data-testid="assertions-tab">
  <h3 id="rt-assertions-title">Assertions</h3>
  {#if assertions.length > 0}
    <table data-testid="assertions-table" aria-labelledby="rt-assertions-title">
      <thead>
        <tr>
          <th><span class="sr-only">Enabled</span></th>
          <th>Expression</th>
          <th>Operator</th>
          <th>Value</th>
          <th><span class="sr-only">Remove</span></th>
        </tr>
      </thead>
      <tbody>
        {#each assertions as a, i (i)}
          <tr data-testid="assertion-row" class:off={!a.enabled}>
            <td>
              <input
                type="checkbox"
                data-testid="assertion-enabled"
                aria-label="Assertion {i + 1} enabled"
                checked={a.enabled}
                onchange={(e) => setAssertion(i, { enabled: e.currentTarget.checked })}
              />
            </td>
            <td>
              <input
                class="mono"
                data-testid="assertion-expression"
                aria-label="Assertion {i + 1} expression"
                placeholder="res.body.id"
                spellcheck="false"
                value={a.expression}
                oninput={(e) => setAssertion(i, { expression: e.currentTarget.value }, true)}
              />
            </td>
            <td>
              <select
                data-testid="assertion-operator"
                aria-label="Assertion {i + 1} operator"
                value={a.operator}
                onchange={(e) => setAssertion(i, { operator: e.currentTarget.value })}
              >
                {#if !known(a.operator)}<option value={a.operator}>{a.operator || "(none)"}</option>{/if}
                {#each OPERATORS as op (op)}<option value={op}>{op}</option>{/each}
              </select>
            </td>
            <td>
              <input
                class="mono"
                data-testid="assertion-value"
                aria-label="Assertion {i + 1} value"
                placeholder={valueHint(a.operator)}
                spellcheck="false"
                disabled={isUnary(a.operator)}
                value={isUnary(a.operator) ? "" : a.value}
                oninput={(e) => setAssertion(i, { value: e.currentTarget.value }, true)}
              />
            </td>
            <td>
              <button
                type="button"
                data-testid="assertion-remove"
                aria-label="Remove assertion {i + 1}"
                onclick={() => onassertions(
                    assertions.filter((_, j) => j !== i),
                    false,
                  )}>×</button
              >
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
  <div class="row">
    <button type="button" data-testid="assertion-add" onclick={() => onassertions([...assertions, newAssertion()], false)}>
      Add assertion
    </button>
    <SuggestAssertionsButton />
    <span class="hint">
      <code>res.status</code>, <code>res.headers.name</code>, <code>res.body.path[0].to</code>, <code>res.responseTime</code> or a
      variable; values may use <code>{"{{variables}}"}</code>.
    </span>
  </div>

  <h3 id="rt-actions-title">Variables from the response</h3>
  {#if actions.length > 0}
    <table data-testid="actions-table" aria-labelledby="rt-actions-title">
      <thead>
        <tr>
          <th><span class="sr-only">Enabled</span></th>
          <th>Variable</th>
          <th>From</th>
          <th>Expression</th>
          <th>Scope</th>
          <th><span class="sr-only">Remove</span></th>
        </tr>
      </thead>
      <tbody>
        {#each actions as a, i (i)}
          <tr data-testid="action-row" class:off={!a.enabled}>
            <td>
              <input
                type="checkbox"
                data-testid="action-enabled"
                aria-label="Action {i + 1} enabled"
                checked={a.enabled}
                onchange={(e) => setAction(i, { enabled: e.currentTarget.checked })}
              />
            </td>
            <td>
              <input
                class="mono"
                data-testid="action-variable"
                aria-label="Action {i + 1} variable name"
                placeholder="token"
                spellcheck="false"
                value={a.variable}
                oninput={(e) => setAction(i, { variable: e.currentTarget.value }, true)}
              />
            </td>
            <td>
              <select
                data-testid="action-method"
                aria-label="Action {i + 1} source"
                value={a.method}
                onchange={(e) => setAction(i, { method: e.currentTarget.value })}
              >
                {#if !METHODS.some((m) => m.method === a.method)}<option value={a.method}>{a.method}</option>{/if}
                {#each METHODS as m (m.method)}<option value={m.method}>{m.label}</option>{/each}
              </select>
            </td>
            <td>
              <input
                class="mono"
                data-testid="action-expression"
                aria-label="Action {i + 1} expression"
                placeholder={METHODS.find((m) => m.method === a.method)?.hint ?? ""}
                spellcheck="false"
                disabled={a.method === "status"}
                value={a.expression}
                oninput={(e) => setAction(i, { expression: e.currentTarget.value }, true)}
              />
            </td>
            <td>
              <select
                data-testid="action-scope"
                aria-label="Action {i + 1} scope"
                value={a.scope}
                onchange={(e) => setAction(i, { scope: e.currentTarget.value as VarAction["scope"] })}
              >
                {#each SCOPES as s (s.scope)}<option value={s.scope}>{s.label}</option>{/each}
              </select>
            </td>
            <td>
              <button
                type="button"
                data-testid="action-remove"
                aria-label="Remove action {i + 1}"
                onclick={() => onactions(
                    actions.filter((_, j) => j !== i),
                    false,
                  )}>×</button
              >
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
  <div class="row">
    <button type="button" data-testid="action-add" onclick={() => onactions([...actions, newAction()], false)}>Add variable</button>
    <span class="hint">
      After each send the value is set for the session (every scope lives in memory; files are never written) and used by the
      next sends.
    </span>
  </div>
</div>

<style>
  .assertions {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
  }
  h3 {
    margin: 0;
    font-size: var(--rt-size-small);
    font-weight: 600;
    color: var(--rt-fg-muted);
    text-transform: uppercase;
  }
  table {
    border-collapse: collapse;
    width: 100%;
  }
  th {
    text-align: left;
    font-weight: normal;
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
    padding: 0 4px;
  }
  td {
    padding: 2px 4px;
  }
  td:first-child,
  td:last-child {
    width: 1%;
  }
  tr.off input:not([type="checkbox"]),
  tr.off select {
    opacity: 0.6;
  }
  input:not([type="checkbox"]),
  select,
  button {
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px 6px;
    box-sizing: border-box;
  }
  input:not([type="checkbox"]) {
    width: 100%;
  }
  input:disabled {
    background: var(--rt-bg-panel);
  }
  .mono {
    font-family: var(--rt-font-mono);
  }
  button {
    cursor: pointer;
    background: var(--rt-bg-panel);
  }
  button:hover {
    background: var(--rt-bg-hover);
  }
  .row {
    display: flex;
    align-items: baseline;
    gap: var(--rt-gap);
    flex-wrap: wrap;
  }
  .hint {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
