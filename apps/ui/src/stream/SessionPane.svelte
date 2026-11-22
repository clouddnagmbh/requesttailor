<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Session pane (wave 8, strand A): shown instead of the HTTP response for WebSocket, SSE
     and gRPC requests. Status of the session, "Send" of the selected message (WebSocket,
     gRPC), "End stream" for gRPC client and bidi streaming, and the message history. -->
<script lang="ts">
  import type { Frame } from "../commands/types";
  import { describeError } from "../response/format";
  import { getApp } from "../state/app.svelte";
  import { messagesOf, selectedMessage } from "./kind";
  import type { SessionsState } from "./sessions.svelte";
  import StreamView from "./StreamView.svelte";

  let {
    path,
    sessions,
    decode,
  }: {
    path: string;
    sessions: SessionsState;
    /** Display text of a frame (gRPC: protobuf → JSON, strand B). */
    decode?: (frame: Frame) => string;
  } = $props();
  const app = getApp();

  const kind = $derived(sessions.kindOf(path));
  const entry = $derived(sessions.entries[path]);
  const message = $derived(selectedMessage(messagesOf(app.drafts[path])));
  const grpcType = $derived.by(() => {
    const p = app.drafts[path]?.protocol;
    return p?.type === "grpc" ? p.method_type : null;
  });
  const streamingClient = $derived(grpcType === "client-streaming" || grpcType === "bidi-streaming");
  const canSend = $derived(
    entry?.status === "open" && message !== null && (grpcType === null || streamingClient || entry.sent === 0),
  );
  const LABEL = { websocket: "WebSocket", sse: "Server-Sent Events", grpc: "gRPC" } as const;

  const closed = $derived.by(() => {
    const events = entry?.events ?? [];
    for (let i = events.length - 1; i >= 0; i--) {
      const e = events[i];
      if (e.event === "closed" || e.event === "error") return e;
    }
    return null;
  });
  const statusText = $derived.by(() => {
    if (!entry) return "Not connected";
    switch (entry.status) {
      case "connecting":
        return "Connecting…";
      case "open":
        return "Connected";
      case "closed": {
        if (closed?.event !== "closed") return "Closed";
        const by = closed.by_client ? "by you" : "by the server";
        const code = closed.code !== null ? ` (${closed.code}${closed.reason ? ` ${closed.reason}` : ""})` : closed.reason ? ` (${closed.reason})` : "";
        return `Closed ${by}${code}`;
      }
      case "error":
        if (closed?.event === "error") return `Failed: ${describeError({ kind: "transport", message: closed.error })}`;
        return `Failed: ${entry.error ?? "unknown error"}`;
    }
  });
</script>

<section class="session" data-testid="session-pane" aria-label="Session">
  <div class="bar">
    <span class="kind">{kind ? LABEL[kind] : ""}</span>
    <span class="status" data-testid="session-status" data-status={entry?.status ?? "idle"} role="status">{statusText}</span>
    <span class="spacer"></span>
    {#if kind !== "sse"}
      <button
        class="button"
        data-testid="session-send"
        disabled={!canSend}
        title={message ? `Send “${message.title || "Message"}”` : "No message to send"}
        onclick={() => sessions.sendSelected(path)}>Send {message?.title ? `“${message.title}”` : "message"}</button
      >
    {/if}
    {#if streamingClient}
      <button class="button" data-testid="session-finish" disabled={entry?.status !== "open"} onclick={() => sessions.finish(path)}
        >End stream</button
      >
    {/if}
  </div>
  {#if entry?.error && entry.status !== "error"}
    <p class="error" role="alert" data-testid="session-error">{entry.error}</p>
  {/if}
  {#if entry}
    <StreamView events={entry.events} {decode} />
  {:else}
    <p class="note">
      {kind === "sse" ? "Connect to receive events." : "Connect, then send the selected message."}
    </p>
  {/if}
</section>

<style>
  .session {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
    height: 100%;
    min-height: 320px;
    box-sizing: border-box;
    padding-top: var(--rt-gap);
    border-top: 1px solid var(--rt-border);
    background: var(--rt-bg);
    color: var(--rt-fg);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
  }
  .kind {
    font-weight: 600;
  }
  .status {
    color: var(--rt-fg-muted);
  }
  .status[data-status="open"] {
    color: var(--rt-success, var(--rt-accent));
  }
  .status[data-status="error"] {
    color: var(--rt-danger);
  }
  .spacer {
    flex: 1;
  }
  .button {
    height: 26px;
    padding: 0 10px;
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    cursor: pointer;
  }
  .button:hover:not(:disabled) {
    background: var(--rt-bg-hover);
  }
  .button:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .error {
    margin: 0;
    color: var(--rt-danger);
  }
  .note {
    margin: 0;
    color: var(--rt-fg-muted);
  }
</style>
