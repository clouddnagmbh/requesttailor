<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- After a response: status, headers and body against the operation's response in the
     collection's spec (`OpenApiApi.validateResponse`). A small chip in the response summary,
     "Matches spec" or "N differences"; the differences (path, expected, got) in a dialog.
     Never blocks anything; nothing shows without a spec or a matching operation. -->
<script lang="ts">
  import type { Backend } from "../commands/backend";
  import type { ResponseCheck, StoredResponse } from "../commands/types";
  import { Chip, Dialog } from "../ui";

  let { backend, path, result }: { backend: Backend; path: string; result: StoredResponse } = $props();

  let check = $state<ResponseCheck | null>(null);
  let open = $state(false);

  $effect(() => {
    const id = result.response_id;
    const p = path;
    check = null;
    if (id == null) return;
    let live = true;
    backend.openapi
      .validateResponse(p, id)
      .then((c) => {
        if (live) check = c;
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  });

  const count = $derived(check?.differences.length ?? 0);
  const PLACE: Record<string, string> = { status: "Status", header: "Header", "content-type": "Content type", body: "Body" };
</script>

{#if check}
  <Chip
    size="sm"
    icon={count === 0 ? "spec-ok" : "warning"}
    tone={count === 0 ? "ok" : "warn"}
    label={count === 0 ? "Matches spec" : `${count} ${count === 1 ? "difference" : "differences"}`}
    title={`Checked against ${check.operation}${check.matched ? ` (response ${check.matched})` : ""}`}
    onclick={() => (open = true)}
    data-testid="spec-badge"
    data-count={count}
  />
  {#if open}
    <Dialog title="Response against the spec" description={`${check.operation} · status ${check.status}${check.matched ? ` · checked against ${check.matched}` : ""}`} icon="spec-ok" size="lg" testid="spec-differences" onclose={() => (open = false)}>
      {#if count === 0}
        <p>Status, headers{check.body_skipped ? "" : " and body"} match what the spec declares.</p>
      {:else}
        <table>
          <thead><tr><th>Where</th><th>Path</th><th>Expected</th><th>Got</th></tr></thead>
          <tbody>
            {#each check.differences as d, i (i)}
              <tr data-testid="spec-difference">
                <td>{PLACE[d.place] ?? d.place}</td>
                <td><code>{d.path}</code></td>
                <td>{d.expected}</td>
                <td><code>{d.got}</code></td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
      {#if check.body_skipped}<p class="muted">The body was not checked: {check.body_skipped}.</p>{/if}
    </Dialog>
  {/if}
{/if}

<style>
  table {
    width: 100%;
    border-collapse: collapse;
    font: var(--rt-font-small);
  }
  th {
    padding: var(--rt-space-3) var(--rt-space-4);
    border-bottom: 1px solid var(--rt-border);
    color: var(--rt-fg-muted);
    font: var(--rt-font-caption);
    text-align: left;
    text-transform: uppercase;
  }
  td {
    padding: var(--rt-space-3) var(--rt-space-4);
    border-bottom: 1px solid var(--rt-border);
    vertical-align: top;
    overflow-wrap: anywhere;
  }
  code {
    font-family: var(--rt-font-mono);
    font-size: 12px;
  }
  .muted {
    margin-top: var(--rt-space-5);
    color: var(--rt-fg-muted);
  }
</style>
