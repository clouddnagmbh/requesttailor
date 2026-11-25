<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Paging rule" (wave 18, strand C): how the next page of a request's responses is found, and
     where the rule is kept: in a file next to the request (`<request>.paging.json`) or as the
     default of the collection. "Test" runs the rule on the response in view for two pages at
     most and says what it found. Saving stages the file like any edit; the user saves it. -->
<script lang="ts">
  import type { ExtensionDialogProps } from "../extensions";
  import type { AppState } from "../state/app.svelte";
  import type { EffectivePaging, PagingRuleScope } from "../commands/types";
  import { Banner, Button, DialogFooter, Select, TextField } from "../ui";
  import { CURSOR_PLACES, RULE_KINDS, draftOf, draftProblem, resultSummary, specOf, type RuleDraft, type RuleKind } from "./model";

  let { close, props }: ExtensionDialogProps = $props();
  const app = $derived(props.app as AppState);
  const requestPath = $derived(props.requestPath as string);
  const responseId = $derived(props.responseId as number | null);
  const current = $derived(props.current as EffectivePaging | null);

  // The dialog opens with the rule in force; later changes of the props do not reset the form.
  // svelte-ignore state_referenced_locally
  let draft = $state<RuleDraft>(draftOf(current?.spec ?? null));
  // svelte-ignore state_referenced_locally
  let scope = $state<PagingRuleScope>(current?.origin.kind === "collection" ? "collection" : "request");
  let error = $state<string | null>(null);
  let tested = $state<string | null>(null);
  let busy = $state(false);
  const problem = $derived(draftProblem(draft));
  const kind = $derived(RULE_KINDS.find((k) => k.value === draft.kind));

  async function save() {
    if (problem) return;
    busy = true;
    error = null;
    try {
      await app.backend.paging.setRule(requestPath, specOf(draft), scope);
      await app.refreshDirty();
      close(true);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  async function remove() {
    busy = true;
    error = null;
    try {
      await app.backend.paging.setRule(requestPath, null, scope);
      await app.refreshDirty();
      close(true);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  // Tests the rule as drafted: stage it, run two pages, and put the saved state back.
  async function test() {
    if (problem || responseId === null) return;
    busy = true;
    error = null;
    tested = null;
    const before = current && current.origin.kind !== "plugin" ? current.spec : null;
    const beforeScope: PagingRuleScope = current?.origin.kind === "collection" ? "collection" : "request";
    try {
      await app.backend.paging.setRule(requestPath, specOf(draft), scope);
      const result = await app.backend.paging.run({
        path: requestPath,
        input: { environment: app.environment, vars: [] },
        mode: "all",
        response_id: responseId,
        limits: { max_pages: 2, max_bytes: 10 * 1024 * 1024, max_seconds: 30 },
      });
      tested = resultSummary(result);
      if (result.response.response_id !== null) await app.backend.releaseResponse(result.response.response_id);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      // The test only borrowed the rule: what was saved before comes back (`Save` keeps it).
      await app.backend.paging.setRule(requestPath, before, beforeScope).catch(() => undefined);
      busy = false;
    }
  }
</script>

<div class="form" data-testid="paging-dialog">
  {#if current?.origin.kind === "plugin"}
    <Banner tone="info">The rule comes from the plugin {current.origin.plugin}. Saving it here makes it yours: it wins from now on.</Banner>
  {/if}
  <Select
    label="How is the next page found?"
    value={draft.kind}
    options={RULE_KINDS.map((k) => ({ value: k.value, label: k.label }))}
    hint={kind?.hint}
    data-testid="paging-kind"
    onchange={(e) => (draft.kind = (e.currentTarget as HTMLSelectElement).value as RuleKind)}
  />
  {#if draft.kind !== "link-header"}
    <TextField label="JSONPath to the next page" bind:value={draft.next} placeholder="$.next" data-testid="paging-next-path" />
  {/if}
  {#if draft.kind === "cursor"}
    <Select
      label="The cursor goes in"
      value={draft.place}
      options={CURSOR_PLACES}
      data-testid="paging-place"
      onchange={(e) => (draft.place = (e.currentTarget as HTMLSelectElement).value as RuleDraft["place"])}
    />
    <TextField label="Parameter name" bind:value={draft.name} placeholder="cursor" data-testid="paging-name" />
  {/if}
  <TextField
    label="Merge the items at (optional)"
    bind:value={draft.merge}
    placeholder="$.items"
    hint="The arrays at this path of every page are appended to the first page's. Without it pages cannot be collected."
    data-testid="paging-merge"
  />
  <Select
    label="Keep the rule"
    value={scope}
    options={[
      { value: "request", label: "For this request (next to its file)" },
      { value: "collection", label: "As the default of the collection" },
    ]}
    data-testid="paging-scope"
    onchange={(e) => (scope = (e.currentTarget as HTMLSelectElement).value as PagingRuleScope)}
  />
  <p class="limits">
    Following pages is limited: 20 pages by default, 1 000 at most; 50 MiB by default, 100 MiB at most; 2 minutes by default, 10 at
    most. It stays on the host of the request and only reads.
  </p>
  {#if problem}<p class="problem" data-testid="paging-problem">{problem}</p>{/if}
  {#if tested}<Banner tone="ok"><span data-testid="paging-tested">Tested: {tested}.</span></Banner>{/if}
  {#if error}<Banner tone="danger"><span data-testid="paging-dialog-error">{error}</span></Banner>{/if}
</div>

<DialogFooter>
  {#snippet start()}
    {#if current && current.origin.kind !== "plugin"}<Button variant="ghost" disabled={busy} data-testid="paging-remove" onclick={remove}>Remove the rule</Button>{/if}
  {/snippet}
  <Button variant="ghost" disabled={busy || problem !== null || responseId === null} data-testid="paging-test" onclick={test}>Test on this response</Button>
  <Button variant="ghost" onclick={() => close(false)}>Cancel</Button>
  <Button variant="primary" disabled={busy || problem !== null} data-testid="paging-save" onclick={save}>Save</Button>
</DialogFooter>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
  }
  .limits {
    margin: 0;
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .problem {
    margin: 0;
    color: var(--rt-danger-fg);
  }
</style>
