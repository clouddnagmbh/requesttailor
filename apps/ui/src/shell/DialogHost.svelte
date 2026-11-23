<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The dialog of an extension (`AppState.showDialog`, wave 14, strand C): title bar, close
     button, Escape, focus kept inside and given back, light and dark. One at a time. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";
  import { getExtensions } from "./extension-tabs";

  const app = getApp();
  const extensions = getExtensions();
  const open = $derived(app.dialog);

  const FOCUSABLE =
    'a[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

  /** Moves focus into the dialog, keeps Tab inside it and gives focus back when it goes. */
  function trap(node: HTMLElement) {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const items = () => [...node.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((el) => el.getClientRects().length > 0);
    // After the content mounted: the first control of the body, else the close button.
    queueMicrotask(() => {
      if (node.contains(document.activeElement)) return;
      const body = node.querySelector<HTMLElement>(".body");
      const first = body ? items().find((el) => body.contains(el)) : undefined;
      (first ?? items()[0] ?? node).focus();
    });
    function onkeydown(e: KeyboardEvent) {
      if (e.key !== "Tab") return;
      const list = items();
      if (list.length === 0) {
        e.preventDefault();
        node.focus();
        return;
      }
      const first = list[0]!;
      const last = list[list.length - 1]!;
      const active = document.activeElement;
      if (e.shiftKey && (active === first || !node.contains(active))) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && (active === last || !node.contains(active))) {
        e.preventDefault();
        first.focus();
      }
    }
    node.addEventListener("keydown", onkeydown);
    return {
      destroy() {
        node.removeEventListener("keydown", onkeydown);
        if (before?.isConnected) before.focus();
      },
    };
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && app.dialog) {
      e.preventDefault();
      e.stopPropagation();
      app.closeDialog();
    }
  }
</script>

<svelte:window onkeydowncapture={onkeydown} />

{#if open && extensions}
  {#key open.seq}
    <div class="backdrop" data-testid="extension-dialog-backdrop">
      <div
        class="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="ext-dialog-title"
        tabindex="-1"
        data-testid="extension-dialog"
        style:--dialog-width="{open.dialog.width ?? 560}px"
        use:trap
      >
        <header>
          <h2 id="ext-dialog-title">{open.dialog.title}</h2>
          <button class="close" type="button" aria-label="Close" title="Close (Esc)" data-testid="extension-dialog-close" onclick={() => app.closeDialog()}>
            <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true"><path d="M2 2l8 8M10 2l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" fill="none" /></svg>
          </button>
        </header>
        <div class="body">
          <open.dialog.component ctx={extensions.ctx} close={(r?: unknown) => app.closeDialog(r)} props={open.dialog.props ?? {}} />
        </div>
      </div>
    </div>
  {/key}
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: var(--rt-bg-overlay);
    z-index: 50;
  }
  .dialog {
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    width: min(var(--dialog-width), calc(100vw - 32px));
    max-height: calc(100vh - 32px);
    background: var(--rt-bg);
    color: var(--rt-fg);
    border: 1px solid var(--rt-border);
    border-radius: calc(var(--rt-radius) * 2);
    box-shadow: var(--rt-shadow);
    outline: none;
    overflow: hidden;
  }
  header {
    flex: none;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--rt-gap);
    height: calc(var(--rt-bar-height) + 8px);
    padding: 0 8px 0 16px;
    background: var(--rt-bg-panel);
    border-bottom: 1px solid var(--rt-border);
  }
  h2 {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: calc(var(--rt-size) + 1px);
    font-weight: 600;
  }
  .close {
    flex: none;
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    padding: 0;
    background: none;
    border: 0;
    border-radius: var(--rt-radius);
    color: var(--rt-fg-muted);
    cursor: pointer;
  }
  .close:hover {
    background: var(--rt-bg-hover);
    color: var(--rt-fg);
  }
  .close:focus-visible {
    outline: 2px solid var(--rt-focus);
    outline-offset: 1px;
  }
  .body {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    padding: 12px 16px 16px;
  }
</style>
