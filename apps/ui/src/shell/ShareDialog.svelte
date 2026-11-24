<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Share" (wave 17, strand C): the request as a link that holds the request itself (nothing is
     uploaded), as a QR code, or as a curl, fetch or Python snippet. Secret values are never in
     any of them; "What is in this link" shows exactly what travels. -->
<script lang="ts">
  import type { CopiedAs, CopyKind, ShareLink } from "../commands/types";
  import type { ExtensionDialogProps } from "../extensions";
  import { getApp } from "../state/app.svelte";
  import { Banner, Button, Checkbox, DialogFooter, SegmentedControl } from "../ui";

  let { close, props }: ExtensionDialogProps = $props();
  const app = getApp();
  // svelte-ignore state_referenced_locally
  const path = props.path as string;
  const TABS = [
    { value: "link", label: "Link", testid: "share-tab-link" },
    { value: "curl", label: "curl", testid: "share-tab-curl" },
    { value: "fetch", label: "fetch", testid: "share-tab-fetch" },
    { value: "python", label: "Python", testid: "share-tab-python" },
  ];

  // svelte-ignore state_referenced_locally
  let tab = $state<string>((props.tab as string | undefined) ?? "link");
  let includeValues = $state(false);
  let resolveVariables = $state(false);
  let showPreview = $state(false);
  let showQr = $state(false);
  let link = $state<ShareLink | null>(null);
  let copy = $state<CopiedAs | null>(null);
  let qr = $state<string | null>(null);
  let qrNote = $state<string | null>(null);
  let error = $state<string | null>(null);
  let done = $state<string | null>(null);
  let seq = 0;

  $effect(() => {
    const mine = ++seq;
    const t = tab;
    const values = includeValues;
    const resolve = resolveVariables;
    error = null;
    done = null;
    void (async () => {
      try {
        if (t === "link") {
          const made = await app.backend.share.link(path, { include_values: values, environment: app.environment });
          if (mine !== seq) return;
          link = made;
          copy = null;
          qr = null;
          qrNote = null;
        } else {
          const made = await app.backend.share.copyAs(path, t as CopyKind, { resolve_variables: resolve, environment: app.environment });
          if (mine !== seq) return;
          copy = made;
          link = null;
        }
      } catch (e) {
        if (mine !== seq) return;
        link = null;
        copy = null;
        error = e instanceof Error ? e.message : String(e);
      }
    })();
  });

  $effect(() => {
    const l = link;
    if (!l || !showQr) {
      qr = null;
      qrNote = null;
      return;
    }
    if (!l.fits_qr) {
      qr = null;
      qrNote = "Too long for a QR code, copy the link or save a file.";
      return;
    }
    let live = true;
    app.backend.share
      .qr(l.text)
      .then((svg) => live && ((qr = svg), (qrNote = null)))
      .catch((e: unknown) => live && ((qr = null), (qrNote = e instanceof Error ? e.message : String(e))));
    return () => {
      live = false;
    };
  });

  async function copyText(text: string, what: string) {
    try {
      await navigator.clipboard.writeText(text);
      done = `${what} copied.`;
    } catch {
      done = null;
      error = "The browser did not allow copying; select the text and copy it.";
    }
  }
</script>

<div class="share" data-testid="share-dialog">
  <SegmentedControl label="Share as" options={TABS} bind:value={tab} />

  {#if error}
    <Banner tone="warn" data-testid="share-error">{error}</Banner>
  {:else if tab === "link" && link}
    <p class="muted" data-testid="share-note">
      The link holds the request itself. Nothing is uploaded, and secret values are never part of it.
    </p>
    <Checkbox label="Include variable values" bind:checked={includeValues} data-testid="share-include-values" />
    <p class="muted small">
      {includeValues
        ? "Values of variables that are not secret travel with the link (see what is in it below)."
        : "Only the names of the variables travel; the other person defines their values."}
    </p>
    <pre class="text" data-testid="share-link-text">{link.text}</pre>
    <p class="muted small" data-testid="share-length">{link.length} characters</p>
    {#if showPreview}
      <pre class="text preview" data-testid="share-preview" aria-label="What is in this link">{link.preview}</pre>
    {/if}
    {#if showQr}
      {#if qr}
        <!-- The SVG is made by the core from the link; it holds no script. -->
        <div class="qr" data-testid="share-qr">{@html qr}</div>
      {:else if qrNote}
        <p class="muted" data-testid="share-qr-note">{qrNote}</p>
      {/if}
    {/if}
  {:else if copy}
    <Checkbox label="Resolve variables" bind:checked={resolveVariables} data-testid="share-resolve" />
    <p class="muted small">
      {resolveVariables
        ? "Values of variables that are not secret are written into the snippet."
        : "Variables stay variables of the tool (environment variables). Secret values are never written."}
    </p>
    <pre class="text code" data-testid="share-snippet">{copy.text}</pre>
    {#each copy.notes as note (note)}<p class="muted small" data-testid="share-snippet-note">{note}</p>{/each}
  {/if}
  {#if done}<p class="ok" role="status" data-testid="share-done">{done}</p>{/if}

  <DialogFooter>
    {#snippet start()}
      {#if tab === "link" && link}
        <Button variant="ghost" size="sm" data-testid="share-toggle-preview" onclick={() => (showPreview = !showPreview)}>
          {showPreview ? "Hide what is in this link" : "What is in this link"}
        </Button>
        <Button variant="ghost" size="sm" data-testid="share-toggle-qr" onclick={() => (showQr = !showQr)}>
          {showQr ? "Hide QR code" : "QR code"}
        </Button>
      {/if}
    {/snippet}
    <Button onclick={() => close()}>Close</Button>
    {#if tab === "link" && link}
      <Button variant="primary" data-testid="share-copy" onclick={() => copyText(link!.text, "Link")}>Copy link</Button>
    {:else if copy}
      <Button variant="primary" data-testid="share-copy" onclick={() => copyText(copy!.text, "Snippet")}>Copy</Button>
    {/if}
  </DialogFooter>
</div>

<style>
  .share {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
  }
  .muted {
    margin: 0;
    color: var(--rt-text-muted, inherit);
  }
  .small {
    font-size: var(--rt-text-sm, 0.85em);
  }
  .ok {
    margin: 0;
    color: var(--rt-ok-fg);
  }
  .text {
    margin: 0;
    padding: var(--rt-space-3);
    max-height: 14rem;
    overflow: auto;
    white-space: pre-wrap;
    word-break: break-all;
    background: var(--rt-surface-sunken, transparent);
    border: 1px solid var(--rt-border, currentColor);
    border-radius: var(--rt-radius-md);
    font-family: var(--rt-font-mono, monospace);
    font-size: var(--rt-text-sm, 0.85em);
  }
  .code {
    word-break: normal;
  }
  .qr {
    width: 15rem;
    max-width: 100%;
    align-self: center;
  }
  .qr :global(svg) {
    width: 100%;
    height: auto;
    display: block;
  }
</style>
