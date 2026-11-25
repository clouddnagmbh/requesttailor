<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The JWT view (wave 18, strand B): header, claims with their meaning, times as date and relative
     time, a countdown to `exp` (once per second, only while the page is visible), and the plain
     words "signature not verified": nothing here checks a signature. Either a token that was
     already found (`info`) or a field to paste one into (palette "Inspect token…"). The value is
     decoded by the core, shown here and kept nowhere. -->
<script lang="ts">
  import { untrack } from "svelte";

  import type { JwtInfo } from "../commands/types";
  import type { ExtensionDialogProps } from "../extensions";
  import { getApp } from "../state/app.svelte";
  import { Badge, Banner, Button, DialogFooter, TextArea } from "../ui";
  import { startCountdown } from "./countdown";
  import { claimText, countdown, scopeWords, sentence, ticksFast, timeText } from "./model";

  let { close, props }: ExtensionDialogProps = $props();
  const app = getApp();
  // svelte-ignore state_referenced_locally
  const given = props as { info?: JwtInfo; where?: string };

  let text = $state("");
  let info = $state<JwtInfo | null>(untrack(() => given.info ?? null));
  let error = $state<string | null>(null);
  let now = $state(Math.floor(Date.now() / 1000));

  async function decode() {
    error = null;
    const value = text.trim();
    if (value === "") {
      info = null;
      return;
    }
    try {
      info = await app.backend.inspect.jwt(value);
    } catch (e) {
      info = null;
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function paste() {
    try {
      text = await navigator.clipboard.readText();
      await decode();
    } catch {
      error = "The browser did not let the page read the clipboard. Paste into the field instead.";
    }
  }

  $effect(() => {
    if (info?.expires_at == null) return;
    now = Math.floor(Date.now() / 1000);
    return startCountdown(() => {
      now = Math.floor(Date.now() / 1000);
    });
  });
</script>

<div class="jwt" data-testid="jwt-view">
  {#if !given.info}
    <TextArea
      bind:value={text}
      label="Token"
      rows={3}
      mono
      placeholder="Paste a JSON Web Token"
      data-testid="jwt-input"
      oninput={decode}
      onpaste={() => queueMicrotask(decode)}
    />
    <div class="paste"><Button size="sm" variant="ghost" data-testid="jwt-paste" onclick={paste}>Paste from clipboard</Button></div>
  {:else if given.where}
    <p class="where">Found in the {given.where}.</p>
  {/if}
  {#if error}<p class="error" role="alert" data-testid="jwt-error">{error}</p>{/if}

  {#if info}
    <p class="unverified" data-testid="jwt-unverified">Signature not verified. This view only reads the token; it does not check who made it.</p>
    {#each info.warnings as w (w.kind)}
      <Banner tone="warn" data-testid="jwt-warning" data-kind={w.kind}>{sentence(w.text)}</Banner>
    {/each}
    {#if info.expires_at != null}
      <p class="countdown" data-testid="jwt-countdown" data-fast={ticksFast(info.expires_at, now)} aria-live="off">{countdown(info.expires_at, now)}</p>
    {/if}
    <h3>Header</h3>
    <pre class="header" data-testid="jwt-header">{JSON.stringify(info.header, null, 2)}</pre>
    <h3>Claims</h3>
    <table class="claims" data-testid="jwt-claims">
      <tbody>
        {#each info.claims as c (c.name)}
          <tr data-claim={c.name}>
            <th scope="row"><code>{c.name}</code>{#if c.meaning}<span class="meaning">{c.meaning}</span>{/if}</th>
            <td>
              {#if c.kind === "time" && typeof c.value === "number"}
                {@const t = timeText(c.value, now)}
                <span class="date">{t.date}</span> <span class="rel">({t.relative})</span>
              {:else if c.name === "scope" && scopeWords(c)}
                {#each scopeWords(c) ?? [] as w (w)}<Badge value={w} />{" "}{/each}
              {:else}
                <span class="value">{claimText(c)}</span>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
  <DialogFooter>
    <Button variant="primary" data-testid="jwt-close" data-autofocus={given.info ? true : undefined} onclick={() => close()}>Close</Button>
  </DialogFooter>
</div>

<style>
  .jwt {
    font-size: var(--rt-size-small);
  }
  .paste {
    margin: var(--rt-space-2) 0 var(--rt-space-4);
  }
  .where,
  .unverified {
    margin: 0 0 var(--rt-space-4);
    color: var(--rt-fg-muted);
  }
  .countdown {
    margin: var(--rt-space-4) 0;
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }
  h3 {
    margin: var(--rt-space-5) 0 var(--rt-space-2);
    font-size: var(--rt-size-small);
  }
  .header {
    margin: 0;
    padding: var(--rt-space-3);
    background: var(--rt-surface-sunken);
    border-radius: var(--rt-radius-md);
    font-family: var(--rt-font-mono);
    overflow: auto;
  }
  .claims {
    width: 100%;
    border-collapse: collapse;
  }
  .claims th,
  .claims td {
    padding: var(--rt-space-2) var(--rt-space-3);
    text-align: left;
    vertical-align: top;
    border-bottom: 1px solid var(--rt-border);
  }
  .claims th {
    font-weight: 400;
    white-space: nowrap;
  }
  .meaning {
    display: block;
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-xs, 0.85em);
  }
  .value {
    overflow-wrap: anywhere;
  }
  .rel {
    color: var(--rt-fg-muted);
  }
  .error {
    color: var(--rt-danger-fg);
  }
</style>
