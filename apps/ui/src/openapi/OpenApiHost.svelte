<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Mounted once by the shell: the OpenAPI import wizard and the run wizard's "Ready to send?"
     panel (`store.svelte.ts`), the send gate on `AppState.sendGate` (`gate.ts`), and dropping a
     spec file anywhere on the window (outside other drop zones) opens the wizard with it. -->
<script lang="ts">
  import type { CollectionInfo } from "../commands/backend";
  import { getApp } from "../state/app.svelte";
  import { Dialog } from "../ui";
  import { installSendGate } from "./gate";
  import OpenApiWizard from "./OpenApiWizard.svelte";
  import ReadyToSend from "./ReadyToSend.svelte";
  import { readSpecFiles, looksLikeOpenApi } from "./source";
  import { closeOpenApiWizard, openapiUi, openOpenApiWizard, showReadyPanel } from "./store.svelte";

  let { onopen }: { onopen: (info: CollectionInfo) => Promise<void> } = $props();
  const app = getApp();

  $effect(() => installSendGate(app, showReadyPanel));

  // A spec dropped on the window (not on a field or another drop zone that took it).
  $effect(() => {
    const over = (e: DragEvent) => {
      if (e.defaultPrevented || !e.dataTransfer?.types.includes("Files")) return;
      e.preventDefault();
    };
    const drop = async (e: DragEvent) => {
      if (e.defaultPrevented || !e.dataTransfer?.files.length || openapiUi.wizard) return;
      e.preventDefault();
      const files = await readSpecFiles(e.dataTransfer.files);
      if (files.length && files.some(looksLikeOpenApi)) openOpenApiWizard(files);
    };
    window.addEventListener("dragover", over);
    window.addEventListener("drop", drop);
    return () => {
      window.removeEventListener("dragover", over);
      window.removeEventListener("drop", drop);
    };
  });

  const panelTitle = $derived(openapiUi.panel ? "Ready to send?" : "");
</script>

{#if openapiUi.wizard}
  {#key openapiUi.wizard.seq}
    <Dialog
      title="Import OpenAPI"
      description="Requests, folders and environments from an OpenAPI or Swagger spec."
      icon="spec"
      size="xl"
      closeOnBackdrop={false}
      testid="openapi-dialog"
      onclose={closeOpenApiWizard}
    >
      <OpenApiWizard {app} initial={openapiUi.wizard.files} onclose={closeOpenApiWizard} {onopen} />
    </Dialog>
  {/key}
{/if}

{#if openapiUi.panel}
  {@const panel = openapiUi.panel}
  {#key panel.seq}
    <Dialog
      title={panelTitle}
      description={`${panel.requirements.operation} · checked against ${panel.requirements.spec}`}
      icon="spec-ok"
      size={640}
      closeOnBackdrop={false}
      testid="ready-dialog"
      onclose={() => panel.resolve("cancel")}
    >
      <ReadyToSend {app} path={panel.path} requirements={panel.requirements} controller={panel.controller} done={panel.resolve} />
    </Dialog>
  {/key}
{/if}
