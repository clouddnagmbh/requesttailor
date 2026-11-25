<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The trace id of a send in the status line ("Trace 4bf9…") with a Copy button. Only there
     when the collection switched trace injection on (or the response carried an id). -->
<script lang="ts">
  import { Button } from "../ui";
  import type { ResponseSlotProps } from "../response/slots";
  import { shortTraceId } from "./model";

  let { response }: ResponseSlotProps = $props();

  const trace = $derived(response.report.trace);
  let copied = $state(false);

  async function copy() {
    if (!trace) return;
    try {
      await navigator.clipboard.writeText(trace.trace_id);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      copied = false;
    }
  }
</script>

{#if trace}
  <span class="trace" data-testid="trace-chip" title={`Trace id ${trace.trace_id}${trace.injected ? " (sent as traceparent)" : ""}`}>
    <span class="label">Trace</span>
    <span class="id" data-testid="trace-id" data-trace-id={trace.trace_id}>{shortTraceId(trace.trace_id)}</span>
    <Button variant="ghost" size="sm" data-testid="trace-copy" onclick={copy}>{copied ? "Copied" : "Copy"}</Button>
  </span>
{/if}

<style>
  .trace {
    display: inline-flex;
    align-items: center;
    gap: var(--rt-space-2);
  }
  .label {
    color: var(--rt-fg-muted);
  }
  .id {
    color: var(--rt-fg);
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
  }
</style>
