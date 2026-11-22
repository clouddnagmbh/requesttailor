<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The GraphQL body of the active request (wave 8, strand B): query with completion and
     validation from the schema, variables (JSON), and the schema browser. The schema comes
     from `Backend.graphql.schema` (introspection sent like the request; cached per URL and
     environment until "Refresh"). Without a schema the editor still works, unchecked. -->
<script lang="ts">
  import type { GraphQLSchema } from "graphql";

  import CodeField from "../editor/CodeField.svelte";
  import type { VarLookup } from "../editor/vars";
  import { getApp } from "../state/app.svelte";
  import { parseSchema, variablesProblem } from "./schema";
  import SchemaBrowser from "./SchemaBrowser.svelte";
  import GraphQlQueryField from "./GraphQlQueryField.svelte";

  let {
    query,
    variables,
    lookup,
    onchange,
  }: {
    query: string;
    variables: string;
    lookup: VarLookup;
    onchange: (query: string, variables: string, typing: boolean) => void;
  } = $props();

  const app = getApp();

  let schema = $state<GraphQLSchema | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let browsing = $state(false);
  /** The request and environment the schema was loaded for. */
  let loadedFor = $state<string | null>(null);

  const varsProblem = $derived(variablesProblem(variables));

  async function load(refresh: boolean) {
    const path = app.active;
    if (!path) return;
    loading = true;
    error = null;
    loadedFor = `${path}\n${app.environment ?? ""}`;
    try {
      const sdl = await app.backend.graphql.schema(path, { environment: app.environment, vars: [] }, refresh);
      schema = parseSchema(sdl);
    } catch (e) {
      schema = null;
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  // Loads from the cache on open and when the request or environment changes; the backend
  // only asks the server the first time per URL and environment.
  $effect(() => {
    const key = `${app.active}\n${app.environment ?? ""}`;
    if (key !== loadedFor) void load(false);
  });
</script>

<div class="graphql" data-testid="graphql-editor">
  <div class="bar">
    <span class="label">Query</span>
    <span class="status" data-testid="graphql-schema-status">
      {#if loading}Loading schema…{:else if schema}Schema loaded{:else if error}No schema{:else}No schema yet{/if}
    </span>
    <button type="button" data-testid="graphql-schema-refresh" disabled={loading} onclick={() => load(true)}>
      Refresh schema
    </button>
    <button
      type="button"
      data-testid="graphql-schema-toggle"
      aria-pressed={browsing}
      disabled={!schema}
      onclick={() => (browsing = !browsing)}
    >
      {browsing ? "Hide schema" : "Browse schema"}
    </button>
  </div>
  {#if error}
    <p class="error" role="alert" data-testid="graphql-schema-error">{error}</p>
  {/if}
  <div class="main" class:with-browser={browsing && schema}>
    <div class="editors">
      <GraphQlQueryField value={query} {lookup} {schema} onchange={(q) => onchange(q, variables, true)} />
      <span class="label">Variables</span>
      <CodeField
        testid="graphql-variables"
        label="GraphQL variables"
        multiline
        lang="json"
        placeholder={'{ "name": "value" }'}
        {lookup}
        value={variables}
        onchange={(v) => onchange(query, v, true)}
      />
      {#if varsProblem}
        <p class="error" data-testid="graphql-variables-problem">{varsProblem}</p>
      {/if}
    </div>
    {#if browsing && schema}
      <SchemaBrowser {schema} />
    {/if}
  </div>
</div>

<style>
  .graphql,
  .editors {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
    min-width: 0;
  }
  .bar {
    display: flex;
    gap: var(--rt-gap);
    align-items: center;
    flex-wrap: wrap;
  }
  .label {
    font-weight: 600;
  }
  .status {
    flex: 1;
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .main {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: var(--rt-gap);
  }
  .main.with-browser {
    grid-template-columns: minmax(0, 3fr) minmax(0, 2fr);
  }
  @media (max-width: 720px) {
    .main.with-browser {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  .error {
    color: var(--rt-danger);
    margin: 0;
    overflow-wrap: anywhere;
  }
</style>
