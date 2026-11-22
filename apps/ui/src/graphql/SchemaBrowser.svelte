<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The schema browser of a GraphQL request (wave 8, strand B): types with their fields,
     arguments, docs and deprecations; search over names, types and descriptions. -->
<script lang="ts">
  import type { GraphQLSchema } from "graphql";

  import { browse } from "./schema";

  let { schema }: { schema: GraphQLSchema } = $props();

  let search = $state("");
  const types = $derived(browse(schema, search));
  let open = $state<Record<string, boolean>>({});
  const searching = $derived(search.trim() !== "");
</script>

<div class="browser" data-testid="graphql-schema-browser">
  <input
    class="search"
    type="search"
    placeholder="Search types and fields"
    aria-label="Search the schema"
    data-testid="graphql-schema-search"
    bind:value={search}
  />
  {#if types.length === 0}
    <p class="hint">Nothing matches “{search}”.</p>
  {/if}
  <ul class="types">
    {#each types as t (t.name)}
      <li class="type" data-testid="graphql-schema-type" data-type={t.name}>
        <button
          type="button"
          class="head"
          aria-expanded={searching || !!open[t.name]}
          onclick={() => (open = { ...open, [t.name]: !open[t.name] })}
        >
          <span class="kind">{t.root ?? t.kind}</span>
          <span class="name">{t.name}</span>
        </button>
        {#if searching || open[t.name]}
          {#if t.description}<p class="doc">{t.description}</p>{/if}
          <ul class="fields">
            {#each t.fields as f (f.name)}
              <li data-testid="graphql-schema-field" data-field={f.name} class:deprecated={f.deprecated !== null}>
                <span class="fname">{f.name}</span>{#if f.args}<span class="args">{f.args}</span>{/if}{#if f.type}<span class="ftype">: {f.type}</span>{/if}
                {#if f.description}<p class="doc">{f.description}</p>{/if}
                {#if f.deprecated !== null}<p class="doc warn">Deprecated: {f.deprecated}</p>{/if}
              </li>
            {/each}
          </ul>
        {/if}
      </li>
    {/each}
  </ul>
</div>

<style>
  .browser {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
    min-width: 0;
    max-height: 420px;
    overflow: auto;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg-panel);
    padding: var(--rt-gap);
  }
  .search {
    font: inherit;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .types {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .head {
    display: flex;
    gap: var(--rt-gap);
    width: 100%;
    background: none;
    border: none;
    padding: 2px 0;
    font: inherit;
    color: inherit;
    cursor: pointer;
    text-align: left;
  }
  .kind {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
    min-width: 6em;
  }
  .name,
  .fields {
    font-family: var(--rt-font-mono);
  }
  .fields {
    padding-left: 1.5em;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .args,
  .ftype {
    color: var(--rt-fg-muted);
  }
  .deprecated .fname {
    text-decoration: line-through;
  }
  .doc,
  .hint {
    margin: 0 0 2px;
    color: var(--rt-fg-muted);
    font-family: var(--rt-font-ui);
    font-size: var(--rt-size-small);
    overflow-wrap: anywhere;
  }
  .warn {
    color: var(--rt-warn);
  }
</style>
