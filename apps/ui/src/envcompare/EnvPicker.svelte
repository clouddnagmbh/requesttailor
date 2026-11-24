<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Which environments to compare (wave 18, strand A): the counterpart of the revision picker.
     Two or more; the first chosen is the reference, the others are compared with it. Production
     environments carry a badge. -->
<script lang="ts">
  import { Badge, Button, Checkbox } from "../ui";
  import { makeReference, toggle } from "./model";

  let {
    environments,
    selected = $bindable([]),
    production = [],
    disabled = false,
  }: {
    /** Every environment of the collection. */
    environments: string[];
    /** The chosen ones, reference first. */
    selected: string[];
    /** Names that are production. */
    production?: string[];
    disabled?: boolean;
  } = $props();
</script>

<fieldset class="picker" data-testid="env-picker" {disabled}>
  <legend>Environments <span class="hint">the first is the reference</span></legend>
  <ul>
    {#each environments as name (name)}
      {@const index = selected.indexOf(name)}
      <li data-testid="env-option" data-env={name} data-selected={index >= 0}>
        <Checkbox label={name} checked={index >= 0} onchange={() => (selected = toggle(selected, name))} data-testid="env-check-{name}" />
        {#if production.includes(name)}<Badge tone="danger" value="PRODUCTION" />{/if}
        {#if index === 0}
          <span class="ref" data-testid="env-reference">reference</span>
        {:else if index > 0}
          <Button size="sm" variant="ghost" data-testid="env-make-reference-{name}" onclick={() => (selected = makeReference(selected, name))}>Make reference</Button>
        {/if}
      </li>
    {/each}
  </ul>
</fieldset>

<style>
  .picker {
    margin: 0;
    padding: 0;
    border: 0;
    min-width: 0;
  }
  legend {
    padding: 0;
    margin-bottom: var(--rt-space-3);
    font: var(--rt-font-caption);
    color: var(--rt-fg-muted);
  }
  .hint {
    margin-left: var(--rt-space-3);
    font-weight: normal;
  }
  ul {
    display: flex;
    flex-wrap: wrap;
    gap: var(--rt-space-3) var(--rt-space-6);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: inline-flex;
    align-items: center;
    gap: var(--rt-space-3);
  }
  .ref {
    color: var(--rt-accent-fg, var(--rt-fg-muted));
    font: var(--rt-font-small);
  }
</style>
