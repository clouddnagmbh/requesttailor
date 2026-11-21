<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- App layout: header, sidebar tree, tabs, editor, response, command palette.
     Owner: wave 3, strand A. Placeholder from the interface commit; composes the other
     strands' components through their props only. -->
<script lang="ts">
  import type { Backend } from "../commands/backend";
  import type { TreeNode } from "../commands/types";
  import EnvironmentManager from "../environments/EnvironmentManager.svelte";
  import EnvironmentSwitcher from "../environments/EnvironmentSwitcher.svelte";
  import RequestEditor from "../editor/RequestEditor.svelte";
  import ResponseView from "../response/ResponseView.svelte";
  import { AppState, provideApp } from "../state/app.svelte";
  import CompanionOffer from "../transport/CompanionOffer.svelte";

  let { backend }: { backend: Backend } = $props();
  // The backend is fixed for the lifetime of the shell.
  // svelte-ignore state_referenced_locally
  const app = provideApp(new AppState(backend));
  let managing = $state(false);

  const requests = (node: TreeNode): (TreeNode & { kind: "request" })[] =>
    node.kind === "request" ? [node] : node.children.flatMap(requests);
</script>

<header>
  <button data-testid="open-collection" onclick={() => app.open()}>Open folder</button>
  <span data-testid="collection-name">{app.collection?.name ?? ""}</span>
  <EnvironmentSwitcher />
  <button onclick={() => (managing = true)}>Environments</button>
  <button data-testid="save" onclick={() => app.save()} disabled={app.dirty.length === 0}>Save</button>
</header>

<nav data-testid="sidebar" aria-label="Collection">
  {#if app.tree}
    {#each requests(app.tree) as r (r.path)}
      <button data-testid="tree-request" onclick={() => app.select(r.path)}>{r.method} {r.name}</button>
    {/each}
  {/if}
</nav>

<main>
  {#if app.active}
    <RequestEditor path={app.active} />
    <button data-testid="send" onclick={() => app.send()}>Send</button>
    <CompanionOffer path={app.active} />
    <ResponseView path={app.active} />
  {/if}
</main>

{#if app.error}<p role="alert">{app.error}</p>{/if}
<EnvironmentManager open={managing} onclose={() => (managing = false)} />
