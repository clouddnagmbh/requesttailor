<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Message history of a session (WebSocket, SSE, gRPC), wave 8. Interface placeholder:
     strand A owns this component (virtual list, filter, pretty JSON); strand B uses it for
     gRPC streams. Props are the contract: `events` in order, `decode` turns a frame into
     display text (gRPC: protobuf → JSON). -->
<script lang="ts">
  import type { Frame, StreamEvent } from "../commands/types";

  let {
    events,
    decode = (frame: Frame) => frame.data,
  }: {
    events: StreamEvent[];
    decode?: (frame: Frame) => string;
  } = $props();
</script>

<ol class="stream" aria-label="Messages">
  {#each events as event, i (i)}
    <li class={event.event}>
      {#if event.event === "message"}
        <span class="dir">{event.direction === "sent" ? "↑" : "↓"}</span>
        <code>{decode(event.frame)}</code>
      {:else if event.event === "opened"}
        Connected ({event.head.status})
      {:else if event.event === "closed"}
        Closed {event.code ?? ""} {event.reason}
      {:else}
        Error: {event.error.kind}
      {/if}
    </li>
  {/each}
</ol>

<style>
  .stream {
    list-style: none;
    margin: 0;
    padding: 0;
    font-family: var(--font-mono, monospace);
  }
  .dir {
    display: inline-block;
    width: 1.5em;
  }
</style>
