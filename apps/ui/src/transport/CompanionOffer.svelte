<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Offers the companion service (`rt agent`) when the browser blocked a request.
     Owner: wave 3, strand G.
     One click when the companion runs and this browser is paired: switch to it and resend.
     Otherwise the same button leads to pairing or to how to start `rt agent`. Nothing is
     switched without the click. -->
<script lang="ts">
  import type { CompanionStatus } from "../commands/backend";
  import { getApp } from "../state/app.svelte";

  let { path }: { path: string } = $props();
  const app = getApp();

  const blocked = $derived.by(() => {
    const e = app.results[path]?.report.error;
    return e?.kind === "transport" && e.message.kind === "blocked";
  });

  type Step = "offer" | "busy" | "pair" | "unavailable";
  let step = $state<Step>("offer");
  let status = $state<CompanionStatus | null>(null);
  let code = $state("");
  let problem = $state<string | null>(null);
  let copied = $state(false);

  // Another request or a successful resend starts over.
  let shownFor: string | null = null;
  $effect(() => {
    if (path !== shownFor || !blocked) {
      shownFor = path;
      step = "offer";
      problem = null;
      code = "";
    }
  });

  /** The command inside the backticks of `howTo`, e.g. `rt agent`; else all of it. */
  const command = $derived(status?.howTo.match(/`([^`]+)`/)?.[1] ?? status?.howTo ?? "rt agent");
  const howTo = $derived((status?.howTo ?? "").split(/`[^`]+`/));

  const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

  async function sendThroughCompanion(): Promise<void> {
    await app.backend.transport.useCompanion(true);
    await app.send(path);
  }

  /** The button: checks the companion, then resends, pairs or explains. */
  async function start(): Promise<void> {
    step = "busy";
    problem = null;
    try {
      status = await app.backend.transport.companion();
      if (!status.available) {
        step = "unavailable";
      } else if (!status.paired) {
        step = "pair";
      } else {
        await sendThroughCompanion();
        step = "offer";
      }
    } catch (e) {
      problem = message(e);
      step = "offer";
    }
  }

  const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

  /** Pairs with a typed code, then resends. */
  async function pairWithCode(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const typed = code.trim();
    if (!typed) return;
    step = "busy";
    problem = null;
    try {
      // A typed code takes the same way as the pairing link that `rt agent` prints: the
      // `#rt-pair=` fragment, which the web shell's companion client picks up.
      location.hash = `rt-pair=${encodeURIComponent(typed)}`;
      for (let i = 0; i < 20; i++) {
        await sleep(150);
        status = await app.backend.transport.companion();
        if (status.paired || !status.available) break;
      }
      if (!status?.available) {
        step = "unavailable";
      } else if (!status.paired) {
        problem = "The companion did not accept this code. Use the newest code `rt agent` printed.";
        step = "pair";
      } else {
        code = "";
        await sendThroughCompanion();
        step = "offer";
      }
    } catch (e) {
      problem = message(e);
      step = "pair";
    }
  }

  async function copy(): Promise<void> {
    try {
      await navigator.clipboard.writeText(command);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      problem = "Copying is not allowed here; select the command and copy it.";
    }
  }
</script>

{#if blocked}
  <aside data-testid="companion-offer" aria-live="polite">
    {#if step === "unavailable"}
      <p data-testid="companion-howto">
        The companion is not running.
        {#each howTo as part, i (i)}{part}{#if i < howTo.length - 1}<code>{command}</code>{/if}{/each}
      </p>
      <div class="actions">
        <button data-testid="companion-copy" onclick={copy}>{copied ? "Copied" : "Copy command"}</button>
        <button class="primary" data-testid="companion-retry" onclick={start}>Retry</button>
      </div>
    {:else if step === "pair"}
      <p>
        Pair this browser with the companion: open the pairing link that <code>rt agent</code> printed,
        then retry, or enter the pairing code.
      </p>
      <form class="actions" onsubmit={pairWithCode}>
        <input
          data-testid="companion-code"
          aria-label="Pairing code"
          placeholder="ABCD-EFGH"
          autocomplete="off"
          spellcheck="false"
          bind:value={code}
        />
        <button class="primary" type="submit" data-testid="companion-pair" disabled={!code.trim()}>
          Pair and send
        </button>
        <button type="button" data-testid="companion-retry" onclick={start}>Retry</button>
      </form>
    {:else}
      <p>The browser blocked this request. Send it through the RequestTailor companion?</p>
      <div class="actions">
        <button class="primary" data-testid="companion-use" disabled={step === "busy"} onclick={start}>
          {step === "busy" ? "Connecting…" : "Send through companion"}
        </button>
      </div>
    {/if}
    {#if problem}<p class="problem" role="alert" data-testid="companion-problem">{problem}</p>{/if}
  </aside>
{/if}

<style>
  aside {
    margin: var(--rt-gap) 0;
    padding: var(--rt-gap) calc(var(--rt-gap) * 1.5);
    border: 1px solid var(--rt-warn);
    border-radius: var(--rt-radius);
    background: var(--rt-bg-panel);
    color: var(--rt-fg);
  }
  p {
    margin: 0 0 var(--rt-gap);
  }
  code {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--rt-gap);
    align-items: center;
  }
  button,
  input {
    font: inherit;
    padding: 4px 10px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    color: var(--rt-fg);
  }
  input {
    font-family: var(--rt-font-mono);
    width: 12ch;
  }
  button:hover:not(:disabled) {
    background: var(--rt-bg-hover);
  }
  button.primary {
    border-color: var(--rt-accent);
    background: var(--rt-accent);
    color: var(--rt-accent-fg);
  }
  button:disabled {
    opacity: 0.6;
  }
  .problem {
    margin: var(--rt-gap) 0 0;
    color: var(--rt-danger);
  }
</style>
