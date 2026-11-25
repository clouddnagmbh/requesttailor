<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The "Next page" chip of the response (wave 18, strand C): shown when a rule for following
     pages applies to the request (its own, the collection's, or a plugin's, which is only
     offered). "Next" loads one page more as a response of its own; "All pages" follows them and
     merges the items into this response, within the limits (the page limit is shown). -->
<script lang="ts">
  import type { ResponseSlotProps } from "../response/slots";
  import type { EffectivePaging, PagingMode } from "../commands/types";
  import { Button } from "../ui";
  import { DEFAULT_MAX_PAGES, limitsOf, originText, rememberRun, runOf } from "./model";
  import { openPagingRule } from "./open";

  let { app, requestPath, response }: ResponseSlotProps = $props();

  let found = $state<EffectivePaging | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  const responseId = $derived(response.response_id);
  const last = $derived(runOf(responseId));

  $effect(() => {
    const id = responseId;
    let current = true;
    app.backend.paging
      .rule(requestPath, id)
      .then((rule) => {
        if (current) found = rule;
      })
      .catch(() => {
        if (current) found = null;
      });
    return () => {
      current = false;
    };
  });

  async function run(mode: PagingMode) {
    if (busy || responseId === null) return;
    busy = true;
    error = null;
    const path = requestPath;
    const previous = app.results[path];
    try {
      const result = await app.backend.paging.run({
        path,
        input: { environment: app.environment, vars: [] },
        mode,
        response_id: responseId,
        limits: mode === "all" ? limitsOf(DEFAULT_MAX_PAGES) : null,
      });
      if (result.response.response_id !== null) rememberRun(result.response.response_id, result, mode);
      app.results[path] = result.response;
      if (previous?.response_id != null) await app.backend.releaseResponse(previous.response_id);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if found}
  <span class="paging" data-testid="paging-chip" data-origin={found.origin.kind}>
    <span class="label" title={originText(found)}>{found.origin.kind === "plugin" ? originText(found) : "Next page"}</span>
    <Button size="sm" variant="ghost" disabled={busy || (last?.pages ?? 0) > 1} data-testid="paging-next" onclick={() => run("next")}>Next</Button>
    <Button
      size="sm"
      variant="ghost"
      disabled={busy || !found.spec.merge}
      title={found.spec.merge ? undefined : "Set a merge path in the rule to collect the pages"}
      data-testid="paging-all"
      onclick={() => run("all")}>All pages (max {DEFAULT_MAX_PAGES})</Button
    >
    <Button size="sm" variant="ghost" data-testid="paging-rule" onclick={() => openPagingRule(app, requestPath, responseId, found)}>Rule…</Button>
    {#if last}<span class="note" data-testid="paging-summary">{last.summary}</span>{/if}
    {#if error}<span class="error" role="alert" data-testid="paging-error">{error}</span>{/if}
  </span>
{/if}

<style>
  .paging {
    display: inline-flex;
    align-items: center;
    gap: var(--rt-space-2);
  }
  .label,
  .note {
    color: var(--rt-fg-muted);
  }
  .error {
    color: var(--rt-danger-fg);
  }
</style>
