<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Around a code editor (wave 16, strand C): the toolbar with the actions extensions add to it
     (`AppExtension.editorActions`), their shortcuts, and `EditorHandle.propose`: the suggested
     text appears as a diff in place of the editor until the user applies or discards it. Without
     actions it adds no visible element. The editor stays mounted while the diff shows. -->
<script module lang="ts">
  import type { EditorTarget } from "../extensions";

  /** Which editor this is; the shell adds the active collection and request. */
  export type EditorKind =
    | { kind: "script"; phase: Extract<EditorTarget, { kind: "script" }>["phase"]; owner?: "request" | "folder" | "collection"; path?: string }
    | { kind: "body"; contentType: string | null }
    | { kind: "field"; field: string };
</script>

<script lang="ts">
  import type { Snippet } from "svelte";

  import type { EditorHandle, EditorSelection, SuggestionNote } from "../extensions";
  import { extensionEditorActions } from "../shell/extension-actions";
  import { getExtensions } from "../shell/extension-tabs";
  import { isMacPlatform } from "../shell/shortcuts";
  import { getApp } from "../state/app.svelte";
  import { Banner, Dropdown, IconButton, Button, type MenuEntry } from "../ui";
  import { matchesKey } from "./key";
  import { textBlock } from "./model";
  import SuggestedEditView from "./SuggestedEditView.svelte";

  let {
    editor,
    text,
    selection,
    replace,
    focus,
    children,
  }: {
    editor: EditorKind;
    text: () => string;
    selection: () => EditorSelection | null;
    /** Replaces the whole text as one undo step of the editor; reports the change to the owner. */
    replace: (next: string) => void;
    focus: () => void;
    children: Snippet;
  } = $props();

  const app = getApp();
  const provided = getExtensions();
  const mac = isMacPlatform();

  const target = $derived<EditorTarget>(
    editor.kind === "script"
      ? {
          kind: "script",
          collection: app.activeCollection ?? "",
          owner: editor.owner ?? "request",
          path: editor.path ?? (editor.owner && editor.owner !== "request" ? "" : (app.active ?? "")),
          phase: editor.phase,
        }
      : editor.kind === "body"
        ? { kind: "body", collection: app.activeCollection ?? "", path: app.active ?? "", contentType: editor.contentType }
        : { kind: "field", collection: app.activeCollection ?? "", path: app.active ?? "", field: editor.field },
  );
  const actions = $derived(extensionEditorActions(provided, target));
  const inline = $derived(actions.filter((a) => a.action.inline));
  const menu = $derived(actions.filter((a) => !a.action.inline));

  const LABEL: Record<string, string> = { "before-request": "Script › Before the request", "after-response": "Script › After the response", tests: "Script › Tests" };
  const label = $derived(target.kind === "script" ? (LABEL[target.phase] ?? "Script") : target.kind === "body" ? "Body" : target.field);

  interface Pending {
    base: string;
    next: string;
    note?: SuggestionNote;
    resolve: (applied: boolean) => void;
  }
  let pending = $state<Pending | null>(null);
  let notice = $state<string | null>(null);

  const handle = (): EditorHandle => ({
    get target() {
      return target;
    },
    text,
    selection,
    propose(next, note) {
      pending?.resolve(false);
      notice = null;
      return new Promise<boolean>((resolve) => {
        pending = { base: text(), next, note, resolve };
      });
    },
  });

  function run(ref: (typeof actions)[number]) {
    try {
      const r = ref.action.run(handle());
      if (r instanceof Promise) r.catch((e: unknown) => (app.error = e instanceof Error ? e.message : String(e)));
    } catch (e) {
      app.error = e instanceof Error ? e.message : String(e);
    }
  }

  function done(applied: boolean) {
    const p = pending;
    pending = null;
    p?.resolve(applied);
    focus();
  }

  function apply() {
    const p = pending;
    if (!p) return;
    if (text() !== p.base) {
      notice = "The text changed while the suggestion was open, so it was not applied.";
      return done(false);
    }
    replace(p.next);
    done(true);
  }

  // Shortcuts: before the editor's own keymap sees the key; the first action to claim a key wins.
  function onkeydown(e: KeyboardEvent) {
    if (pending) return;
    const hit = actions.find((a) => a.action.shortcut && matchesKey(a.action.shortcut, e, mac));
    if (!hit) return;
    e.preventDefault();
    e.stopPropagation();
    run(hit);
  }

  const items = $derived<MenuEntry[]>(
    menu.map((a) => ({ id: a.key, label: a.action.label, icon: a.action.icon, shortcut: a.action.shortcut?.replace("-", "+"), run: () => run(a) })),
  );

  $effect(() => () => pending?.resolve(false));
</script>

  <!-- The wrapper only routes the keys of the editor inside it. -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="shell" data-testid="editor-shell" onkeydowncapture={onkeydown}>
    {#if actions.length > 0}
      <div class="bar" role="toolbar" aria-label="{label} actions">
        <span class="spacer"></span>
        {#each inline as a (a.key)}
          <Button size="sm" variant="ghost" icon={a.action.icon ?? "spark"} data-testid="editor-action" data-action={a.key} disabled={pending !== null} onclick={() => run(a)}>
            {a.action.label}
          </Button>
        {/each}
        {#if menu.length > 0}
          <Dropdown {items} menuLabel="{label} actions" placement="bottom-end">
            {#snippet trigger(props)}
              <IconButton {...props} icon="more" label="More actions" size="sm" data-testid="editor-actions-menu" disabled={pending !== null} />
            {/snippet}
          </Dropdown>
        {/if}
      </div>
    {/if}
    {#if notice}
      <Banner tone="warn" dismissible ondismiss={() => (notice = null)} data-testid="suggest-stale">{notice}</Banner>
    {/if}
    <div class="host" hidden={pending !== null}>{@render children()}</div>
    {#if pending}
      <SuggestedEditView
        mode="inline"
        blocks={[textBlock(label, pending.base, pending.next)]}
        note={pending.note}
        onapply={apply}
        ondiscard={() => done(false)}
      />
    {/if}
  </div>

<style>
  .shell {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
    min-width: 0;
    flex: 1;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: var(--rt-space-2);
    min-height: var(--rt-control-sm);
  }
  .spacer {
    flex: 1;
  }
  .host {
    display: flex;
    min-width: 0;
  }
  .host[hidden] {
    display: none;
  }
</style>
