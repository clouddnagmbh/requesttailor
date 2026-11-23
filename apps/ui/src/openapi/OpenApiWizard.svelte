<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The OpenAPI import wizard: Source (file, folder, zip, URL, pasted text) → Overview →
     Choose (operations, grouping, names, examples, servers as environments, auth preview) →
     Target (new collection, or into the open one as a folder; keep the spec for the contract
     check and "Ready to send?") → Result (the loss report in plain words, open the first
     request). Nothing is written before "Import"; the plan is converted by the core
     (`OpenApiApi.plan`) and written like any import (`ImportApi.write` / `writeInto`). -->
<script lang="ts">
  import { onDestroy, untrack } from "svelte";

  import type { CollectionInfo, ImportSource } from "../commands/backend";
  import type { FolderImport, ImportPlan, SpecOverview, TreeNode } from "../commands/types";
  import { groupLosses } from "../import/report";
  import type { AppState } from "../state/app.svelte";
  import {
    Badge,
    Banner,
    Button,
    Card,
    Checkbox,
    DialogFooter,
    EmptyState,
    Icon,
    Progress,
    SegmentedControl,
    Select,
    Switch,
    TextArea,
    TextField,
  } from "../ui";
  import {
    filterGroups,
    groupOperations,
    groupState,
    initialChoices,
    requestName,
    serverErrors,
    setMany,
    summaryOf,
    toOptions,
    type Choices,
  } from "./choose";
  import { fetchedSources, pastedSpec, readSpecFiles, sizeText, validUrl } from "./source";

  let {
    app,
    initial = null,
    onclose,
    onopen,
  }: {
    app: AppState;
    initial?: ImportSource[] | null;
    onclose: () => void;
    /** Opens a new collection the import wrote (the shell's adopt). */
    onopen: (info: CollectionInfo) => Promise<void>;
  } = $props();

  type Step = "source" | "overview" | "choose" | "target" | "result";
  const STEPS: { id: Step; label: string }[] = [
    { id: "source", label: "Source" },
    { id: "overview", label: "Overview" },
    { id: "choose", label: "Choose" },
    { id: "target", label: "Target" },
    { id: "result", label: "Done" },
  ];

  let step = $state<Step>("source");
  let busy = $state<null | "reading" | "fetching" | "importing">(null);
  let error = $state<string | null>(null);
  let files = $state<ImportSource[]>([]);
  let origin = $state("");
  let spec = $state<SpecOverview | null>(null);
  let choices = $state<Choices | null>(null);
  let query = $state("");
  let url = $state("");
  let urlError = $state<string | null>(null);
  let pasteText = $state("");
  let dragging = $state(false);
  let fileInput = $state<HTMLInputElement | null>(null);
  let folderInput = $state<HTMLInputElement | null>(null);
  let target = $state<"new" | "folder">("new");
  let folderName = $state("");
  let plan = $state<ImportPlan | null>(null);
  let done = $state<{ plan: ImportPlan; into: FolderImport | null; collection: string; first: string | null } | null>(null);

  const stepIndex = $derived(STEPS.findIndex((s) => s.id === step));
  const groups = $derived(spec && choices ? groupOperations(spec, choices.grouping) : []);
  const shown = $derived(filterGroups(groups, query));
  const counts = $derived(spec && choices ? summaryOf(spec, choices) : null);
  const serverProblems = $derived(choices ? serverErrors(choices.servers) : {});
  const canChoose = $derived(!!choices && choices.selected.size > 0 && Object.keys(serverProblems).length === 0);
  const tagCounts = $derived(spec ? groupOperations(spec, spec.tags.length ? "tag" : "path") : []);
  const maxCount = $derived(Math.max(1, ...tagCounts.map((g) => g.operations.length)));
  const lossGroups = $derived(done ? groupLosses([...done.plan.report.losses, ...(done.into?.losses ?? [])]) : []);

  const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

  async function read(sources: ImportSource[], from: string) {
    if (sources.length === 0 || busy) return;
    busy = "reading";
    error = null;
    try {
      const overview = await app.backend.openapi.inspect(sources);
      files = sources;
      origin = from;
      spec = overview;
      choices = initialChoices(overview);
      folderName = overview.title;
      query = "";
      step = "overview";
    } catch (e) {
      error = message(e);
    } finally {
      busy = null;
    }
  }

  $effect(() => {
    const start = initial;
    if (start && start.length) untrack(() => void read(start, start.length === 1 ? start[0]!.name : `${start.length} files`));
  });

  async function picked(list: FileList | null) {
    if (list && list.length) {
      const sources = await readSpecFiles(list);
      await read(sources, sources.length === 1 ? sources[0]!.name : `${sources.length} files`);
    }
    if (fileInput) fileInput.value = "";
    if (folderInput) folderInput.value = "";
  }

  async function ondrop(e: DragEvent) {
    e.preventDefault();
    dragging = false;
    await picked(e.dataTransfer?.files ?? null);
  }

  async function fetchUrl() {
    urlError = validUrl(url);
    if (urlError || busy) return;
    busy = "fetching";
    error = null;
    let sources: ImportSource[];
    try {
      const fetched = await app.backend.openapi.fetch(url.trim());
      sources = fetchedSources(fetched);
    } catch (e) {
      error = message(e);
      busy = null;
      return;
    }
    busy = null;
    await read(sources, url.trim());
  }

  async function readPasted() {
    if (pasteText.trim() === "") return;
    await read([pastedSpec(pasteText)], "pasted text");
  }

  function toggleGroup(ops: { key: string }[], on: boolean) {
    if (!choices) return;
    choices.selected = setMany(choices.selected, ops as never, on);
  }

  function toggleOne(key: string, on: boolean) {
    if (!choices) return;
    const next = new Set(choices.selected);
    if (on) next.add(key);
    else next.delete(key);
    choices.selected = next;
  }

  function firstRequest(node: TreeNode | null, under: string): string | null {
    if (!node) return null;
    if (node.kind === "request") return under === "" || node.path.startsWith(`${under}/`) ? node.path : null;
    for (const child of node.children) {
      const found = firstRequest(child, under);
      if (found) return found;
    }
    return null;
  }

  async function runImport() {
    if (!spec || !choices || busy) return;
    busy = "importing";
    error = null;
    try {
      const options = toOptions(spec, choices);
      if (target === "new") options.collection_name = choices.collectionName.trim() || spec.title;
      plan = await app.backend.openapi.plan(files, options);
      if (target === "folder" && app.collection) {
        await app.settled();
        const into = await app.backend.imports.writeInto(plan.id, folderName.trim() || null);
        await app.reload();
        await app.reloadEnvironments();
        done = { plan, into, collection: app.collection?.name ?? "", first: firstRequest(app.tree, into.folder) };
      } else {
        const info = await app.backend.imports.write(plan.id);
        if (!info) {
          // Folder picker cancelled: the plan stays for another try.
          busy = null;
          return;
        }
        await onopen(info);
        done = { plan, into: null, collection: info.name, first: firstRequest(app.tree, "") };
      }
      plan = null;
      step = "result";
    } catch (e) {
      error = message(e);
    } finally {
      busy = null;
    }
  }

  async function openFirst() {
    if (done?.first) await app.select(done.first);
    onclose();
  }

  function back() {
    error = null;
    if (step === "overview") step = "source";
    else if (step === "choose") step = "overview";
    else if (step === "target") step = "choose";
  }

  onDestroy(() => {
    const id = plan?.id;
    if (id !== undefined) void app.backend.imports.discard(id).catch(() => {});
  });

  const groupingOptions = [
    { value: "tag", label: "By tag" },
    { value: "path", label: "By path" },
    { value: "flat", label: "Flat" },
  ];
  const namingOptions = [
    { value: "summary", label: "Summary" },
    { value: "operation-id", label: "operationId" },
  ];
  const exampleOptions = [
    { value: "spec", label: "Spec examples" },
    { value: "schema", label: "From schema" },
  ];
  const specKind = (v: string) => (v.startsWith("2") ? `Swagger ${v}` : `OpenAPI ${v}`);
  const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
</script>

<div class="wizard" data-testid="openapi-wizard" data-step={step}>
  <ol class="steps" aria-label="Steps">
    {#each STEPS as s, i (s.id)}
      <li class:current={i === stepIndex} class:past={i < stepIndex} aria-current={i === stepIndex ? "step" : undefined}>
        <span class="dot">{#if i < stepIndex}<Icon name="check" size="sm" />{:else}{i + 1}{/if}</span>
        <span class="label">{s.label}</span>
      </li>
    {/each}
  </ol>

  {#if error}
    <Banner tone="danger" title="That did not work" data-testid="openapi-error">{error}</Banner>
  {/if}

  {#if step === "source"}
    <section class="source">
      <div
        class="drop"
        class:dragging
        role="region"
        aria-label="Drop an OpenAPI file"
        data-testid="openapi-drop"
        ondragover={(e) => {
          e.preventDefault();
          dragging = true;
        }}
        ondragleave={() => (dragging = false)}
        ondrop={ondrop}
      >
        <span class="drop-icon"><Icon name="spec" size="lg" /></span>
        <div class="drop-text">
          <strong>Drop an OpenAPI or Swagger file</strong>
          <span>YAML or JSON, OpenAPI 3.0–3.2 or Swagger 2.0. A multi-file spec as a folder or .zip.</span>
        </div>
        <div class="drop-actions">
          <Button icon="file" onclick={() => fileInput?.click()} disabled={!!busy} data-testid="openapi-pick">Choose file…</Button>
          <Button icon="folder-open" variant="ghost" onclick={() => folderInput?.click()} disabled={!!busy}>Folder…</Button>
        </div>
        <input
          bind:this={fileInput}
          type="file"
          multiple
          accept=".yaml,.yml,.json,.zip,application/json,application/yaml,application/zip"
          hidden
          data-testid="openapi-file"
          onchange={(e) => picked((e.currentTarget as HTMLInputElement).files)}
        />
        <input
          bind:this={folderInput}
          type="file"
          hidden
          webkitdirectory
          onchange={(e) => picked((e.currentTarget as HTMLInputElement).files)}
        />
      </div>

      <div class="or"><span>or fetch from a URL</span></div>
      <form
        class="url-row"
        onsubmit={(e) => {
          e.preventDefault();
          void fetchUrl();
        }}
      >
        <div class="grow">
          <TextField
            bind:value={url}
            icon="link"
            placeholder="https://api.example.com/openapi.yaml"
            aria-label="Spec URL"
            mono
            error={urlError ?? undefined}
            data-testid="openapi-url"
            oninput={() => (urlError = null)}
          />
        </div>
        <Button type="submit" loading={busy === "fetching"} disabled={!!busy || url.trim() === ""} data-testid="openapi-fetch">Fetch</Button>
      </form>
      {#if busy === "fetching"}<Progress label="Fetching the spec" size="sm" />{/if}

      <div class="or"><span>or paste the spec</span></div>
      <TextArea bind:value={pasteText} rows={5} mono placeholder={"openapi: 3.1.0\ninfo:\n  title: …"} aria-label="Pasted spec" data-testid="openapi-paste" />
      <div class="paste-row">
        <Button onclick={readPasted} disabled={!!busy || pasteText.trim() === ""} data-testid="openapi-read-paste">Read pasted spec</Button>
      </div>
      {#if busy === "reading"}<Progress label="Reading the spec" size="sm" />{/if}
    </section>
    <DialogFooter>
      <Button variant="ghost" onclick={onclose}>Cancel</Button>
    </DialogFooter>
  {:else if step === "overview" && spec}
    <section class="overview" data-testid="openapi-overview">
      <header class="spec-head">
        <div class="spec-title">
          <h3>{spec.title}</h3>
          <div class="badges">
            {#if spec.version}<Badge value={`v${spec.version}`} />{/if}
            <Badge value={specKind(spec.spec_version)} tone="info" />
            {#if spec.files > 1}<Badge value={plural(spec.files, "file", "files")} />{/if}
          </div>
        </div>
        <p class="from"><Icon name="file-text" size="sm" /> {origin}{#if files.length === 1}&ensp;·&ensp;{sizeText(files[0]!.data.length)}{/if}</p>
        {#if spec.description}<p class="description">{spec.description}</p>{/if}
      </header>

      <div class="stats">
        <div class="stat"><span class="n" data-testid="stat-operations">{spec.operations.length}</span><span>operations</span></div>
        <div class="stat"><span class="n">{spec.tags.length}</span><span>tags</span></div>
        <div class="stat"><span class="n" data-testid="stat-servers">{spec.servers.length}</span><span>servers</span></div>
        <div class="stat"><span class="n">{spec.security.length}</span><span>auth schemes</span></div>
      </div>

      <div class="columns">
        <div>
          <h4>Operations per {spec.tags.length ? "tag" : "path"}</h4>
          <ul class="bars">
            {#each tagCounts as g (g.key)}
              <li>
                <span class="bar-label" title={g.description ?? undefined}>{g.label || "Other"}</span>
                <span class="bar"><span style:width="{(g.operations.length / maxCount) * 100}%"></span></span>
                <span class="bar-n">{g.operations.length}</span>
              </li>
            {/each}
          </ul>
        </div>
        <div>
          <h4>Auth</h4>
          {#if spec.security.length === 0}
            <p class="muted">No security schemes: requests are sent without auth.</p>
          {:else}
            <ul class="auth">
              {#each spec.security as s (s.name)}
                <li>
                  <Icon name={s.maps_to ? "key" : "warning"} size="sm" />
                  <div>
                    <strong>{s.name}</strong> <span class="muted">{s.detail}</span>
                    <div class="muted small">{s.maps_to ?? "Not imported: set auth by hand"}</div>
                  </div>
                </li>
              {/each}
            </ul>
          {/if}
          <h4>Servers</h4>
          {#if spec.servers.length === 0}
            <p class="muted">No server: set <code>baseUrl</code> after the import.</p>
          {:else}
            <ul class="servers-list">
              {#each spec.servers as s (s.url)}<li>{#if s.description}<span class="server-name">{s.description}</span>{/if}<code>{s.url}</code></li>{/each}
            </ul>
          {/if}
        </div>
      </div>
      {#if spec.losses.length}
        <Banner tone="warn" title="Some parts could not be read">
          <ul class="plain">{#each spec.losses as l, i (i)}<li>{l.detail}</li>{/each}</ul>
        </Banner>
      {/if}
    </section>
    <DialogFooter>
      <Button variant="ghost" onclick={back}>Back</Button>
      <Button variant="primary" onclick={() => (step = "choose")} data-testid="openapi-next">Choose operations</Button>
    </DialogFooter>
  {:else if step === "choose" && spec && choices}
    <section class="choose">
      <div class="ops">
        <div class="ops-tools">
          <div class="grow"><TextField bind:value={query} icon="search" size="sm" placeholder="Filter operations" aria-label="Filter operations" data-testid="openapi-search" /></div>
          <SegmentedControl options={groupingOptions} bind:value={choices.grouping} label="Group requests" size="sm" />
        </div>
        <div class="ops-bar">
          <span class="muted small" data-testid="openapi-selected">{choices.selected.size} of {spec.operations.length} selected</span>
          <span class="spacer"></span>
          <Button size="sm" variant="ghost" onclick={() => toggleGroup(shown.flatMap((g) => g.operations), true)} data-testid="openapi-all">Select all</Button>
          <Button size="sm" variant="ghost" onclick={() => toggleGroup(shown.flatMap((g) => g.operations), false)} data-testid="openapi-none">None</Button>
        </div>
        <div class="tree" role="group" aria-label="Operations">
          {#if shown.length === 0}
            <EmptyState icon="search" title="No operation matches" text="Try another word or clear the filter." size="sm" />
          {/if}
          {#each shown as g (g.key)}
            {@const st = groupState(g, choices.selected)}
            {#if g.label}
              <div class="group-row" data-testid="openapi-group" data-group={g.key}>
                <Checkbox
                  checked={st === "all"}
                  indeterminate={st === "some"}
                  label={g.label}
                  onchange={(e) => toggleGroup(g.operations, (e.currentTarget as HTMLInputElement).checked)}
                />
                <span class="muted small">{g.operations.length}</span>
              </div>
            {/if}
            {#each g.operations as op (op.key)}
              <label class="op-row" class:nested={!!g.label} data-testid="openapi-op" data-key={op.key}>
                <input
                  type="checkbox"
                  class="op-check"
                  checked={choices.selected.has(op.key)}
                  onchange={(e) => toggleOne(op.key, (e.currentTarget as HTMLInputElement).checked)}
                />
                <Badge kind="method" value={op.method} />
                <code class="path">{op.path}</code>
                <span class="name" class:deprecated={op.deprecated}>{requestName(op, choices.naming)}</span>
                {#if op.deprecated}<Badge value="deprecated" tone="warn" />{/if}
              </label>
            {/each}
          {/each}
        </div>
      </div>
      <aside class="settings">
        <div class="setting">
          <span class="caption">Request names</span>
          <SegmentedControl options={namingOptions} bind:value={choices.naming} label="Request names" size="sm" />
        </div>
        <div class="setting">
          <span class="caption">Bodies and parameters</span>
          <SegmentedControl options={exampleOptions} bind:value={choices.examples} label="Examples" size="sm" />
        </div>
        <div class="setting">
          <span class="caption">Servers become environments</span>
          {#if choices.servers.length === 0}
            <p class="muted small">The spec names no server. Set <code>baseUrl</code> after the import.</p>
          {/if}
          {#each choices.servers as row (row.index)}
            <div class="server" data-testid="openapi-server">
              <Checkbox bind:checked={row.include} aria-label={`Environment for ${row.url}`} />
              <div class="server-fields">
                <TextField bind:value={row.name} size="sm" disabled={!row.include} aria-label="Environment name" error={serverProblems[row.index]} data-testid="openapi-env-name" />
                <code class="server-url" title={row.url}>{row.url}</code>
              </div>
            </div>
          {/each}
          {#if choices.servers.length > 1}
            <Select
              size="sm"
              label="Collection default (baseUrl)"
              value={String(choices.defaultServer ?? 0)}
              options={choices.servers.map((r) => ({ value: String(r.index), label: r.name || r.url }))}
              onchange={(e) => choices && (choices.defaultServer = Number((e.currentTarget as HTMLSelectElement).value))}
            />
          {/if}
        </div>
        {#if spec.security.length}
          <div class="setting">
            <span class="caption">Auth</span>
            {#each spec.security as s (s.name)}
              <p class="map small"><Icon name={s.maps_to ? "key" : "warning"} size="sm" /> <span><strong>{s.name}</strong> → {s.maps_to ?? "not imported"}</span></p>
            {/each}
            <p class="muted small">Secret values go to the keychain, never into files.</p>
          </div>
        {/if}
      </aside>
    </section>
    <DialogFooter>
      {#snippet start()}
        {#if counts}<span class="muted small">{plural(counts.requests, "request", "requests")}, {plural(counts.folders, "folder", "folders")}, {plural(counts.environments, "environment", "environments")}</span>{/if}
      {/snippet}
      <Button variant="ghost" onclick={back}>Back</Button>
      <Button variant="primary" disabled={!canChoose} onclick={() => (step = "target")} data-testid="openapi-next">Next</Button>
    </DialogFooter>
  {:else if step === "target" && spec && choices}
    <section class="target">
      <div class="tiles">
        <Card
          icon="collection"
          title="New collection"
          description="A new folder with the requests and environments."
          selected={target === "new"}
          onclick={() => (target = "new")}
          data-testid="openapi-target-new"
        />
        <Card
          icon="folder"
          title={app.collection ? `Into ${app.collection.name}` : "Into the open collection"}
          description={app.collection ? "As a new folder, unsaved until you save." : "Open a collection first."}
          selected={target === "folder"}
          onclick={() => app.collection && (target = "folder")}
          aria-disabled={!app.collection}
          data-testid="openapi-target-folder"
        />
      </div>
      {#if target === "new"}
        <TextField bind:value={choices.collectionName} label="Collection name" hint="You choose the parent folder next; the collection gets its own folder inside it." data-testid="openapi-collection-name" />
      {:else}
        <TextField bind:value={folderName} label="Folder name" hint="Taken names get “ 2”." data-testid="openapi-folder-name" />
      {/if}
      <Switch
        bind:checked={choices.keepSpec}
        label="Keep the spec in the collection"
        description="Saves it as openapi.yaml (or .json) and names it in opencollection.yml, so the contract check and “Ready to send?” know every operation."
        data-testid="openapi-keep-spec"
      />
    </section>
    <DialogFooter>
      <Button variant="ghost" onclick={back}>Back</Button>
      <Button variant="primary" icon="import" loading={busy === "importing"} disabled={!!busy} onclick={runImport} data-testid="openapi-import">
        {target === "new" ? "Choose folder and import" : "Import"}
      </Button>
    </DialogFooter>
  {:else if step === "result" && done}
    <section class="result" data-testid="openapi-result">
      <div class="result-head">
        <span class="ok-icon"><Icon name="success" size="lg" /></span>
        <div>
          <h3>Imported {plural(done.into?.requests ?? done.plan.report.requests, "request", "requests")} into {done.into ? done.into.name : done.collection}</h3>
          <p class="muted">
            {plural(done.into?.folders ?? done.plan.report.folders, "folder", "folders")}{#if done.into}, unsaved until you save{/if}.
            {#if done.into?.contract_spec}The spec is kept as <code>{done.into.contract_spec}</code>.{/if}
          </p>
        </div>
      </div>
      {#if done.into && (done.into.environments_added.length || done.into.environments_extended.length)}
        <div class="envs" data-testid="openapi-result-envs">
          <span class="caption">Environments</span>
          {#each done.into.environments_added as e (e)}<Badge value={`${e} (new)`} tone="ok" />{/each}
          {#each done.into.environments_extended as e (e)}<Badge value={`${e} (variables added)`} />{/each}
        </div>
      {:else if !done.into && choices}
        <div class="envs" data-testid="openapi-result-envs">
          <span class="caption">Environments</span>
          {#each choices.servers.filter((r) => r.include) as r (r.index)}<Badge value={r.name} tone="ok" />{/each}
        </div>
      {/if}
      {#if lossGroups.length === 0}
        <Banner tone="ok" title="Nothing was lost">Every operation came over as it is in the spec.</Banner>
      {:else}
        <div class="losses" data-testid="openapi-losses">
          <span class="caption">What to look at</span>
          {#each lossGroups as g (g.kind)}
            <details>
              <summary><strong>{g.label}</strong> <span class="muted small">{g.losses.length}</span></summary>
              <ul class="plain">
                {#each g.losses as l, i (i)}
                  <li><span class="where">{l.location === "collection" ? "Collection" : l.location}</span> {l.detail}</li>
                {/each}
              </ul>
            </details>
          {/each}
        </div>
      {/if}
    </section>
    <DialogFooter>
      <Button variant="ghost" onclick={onclose}>Done</Button>
      <Button variant="primary" icon="send" disabled={!done.first} onclick={openFirst} data-testid="openapi-open-first">Open first request</Button>
    </DialogFooter>
  {/if}
</div>

<style>
  .wizard {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-6);
    min-height: 420px;
  }
  /* The footer stays at the bottom edge while a short step leaves room above it. */
  .wizard > :global(.rt-dialog-footer) {
    margin-top: auto;
  }
  .steps {
    display: flex;
    gap: var(--rt-space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .steps li {
    display: flex;
    flex: 1;
    align-items: center;
    gap: var(--rt-space-3);
    padding-bottom: var(--rt-space-3);
    border-bottom: 2px solid var(--rt-border);
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .steps li.past {
    border-color: var(--rt-accent);
  }
  .steps li.current {
    border-color: var(--rt-accent);
    color: var(--rt-fg);
    font-weight: 600;
  }
  .dot {
    display: inline-grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: var(--rt-radius-pill);
    background: var(--rt-surface-sunken);
    font-size: 11px;
  }
  .current .dot,
  .past .dot {
    background: var(--rt-accent-solid);
    color: var(--rt-accent-solid-fg);
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .spacer {
    flex: 1;
  }
  .muted {
    color: var(--rt-fg-muted);
  }
  .small {
    font: var(--rt-font-small);
  }
  .caption {
    display: block;
    margin-bottom: var(--rt-space-3);
    color: var(--rt-fg-muted);
    font: var(--rt-font-caption);
    text-transform: uppercase;
  }
  h3 {
    margin: 0;
    font: var(--rt-font-title);
    font-size: 16px;
  }
  h4 {
    margin: 0 0 var(--rt-space-4);
    color: var(--rt-fg-muted);
    font: var(--rt-font-caption);
    text-transform: uppercase;
  }
  code {
    font-family: var(--rt-font-mono);
    font-size: 12px;
  }
  ul.plain {
    margin: 0;
    padding-left: var(--rt-space-6);
  }
  ul.plain li + li {
    margin-top: var(--rt-space-2);
  }

  /* Source */
  .source {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-5);
  }
  .drop {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    gap: var(--rt-space-5);
    padding: var(--rt-space-7) var(--rt-space-6);
    border: 1px dashed var(--rt-border-strong);
    border-radius: var(--rt-radius-lg);
    background: var(--rt-surface-2);
    transition: border-color var(--rt-dur-fast) var(--rt-ease), background var(--rt-dur-fast) var(--rt-ease);
  }
  .drop.dragging {
    border-color: var(--rt-accent);
    background: var(--rt-accent-subtle);
  }
  .drop-icon {
    display: inline-grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border-radius: var(--rt-radius-md);
    background: var(--rt-accent-subtle);
    color: var(--rt-accent-subtle-fg);
  }
  .drop-text {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-1);
    min-width: 0;
  }
  .drop-text span {
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .drop-actions {
    display: flex;
    gap: var(--rt-space-3);
  }
  .or {
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
    color: var(--rt-fg-muted);
    font: var(--rt-font-caption);
    text-transform: uppercase;
  }
  .or::before,
  .or::after {
    flex: 1;
    height: 1px;
    background: var(--rt-border);
    content: "";
  }
  .url-row {
    display: flex;
    align-items: flex-start;
    gap: var(--rt-space-4);
  }
  .paste-row {
    display: flex;
    justify-content: flex-end;
    margin-top: calc(-1 * var(--rt-space-3));
  }

  /* Overview */
  .overview {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-6);
  }
  .spec-title {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-4);
  }
  .badges {
    display: flex;
    gap: var(--rt-space-2);
  }
  .from {
    display: flex;
    align-items: center;
    gap: var(--rt-space-2);
    margin: var(--rt-space-2) 0 0;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
    overflow-wrap: anywhere;
  }
  .description {
    display: -webkit-box;
    margin: var(--rt-space-4) 0 0;
    overflow: hidden;
    color: var(--rt-fg-muted);
    white-space: pre-line;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 4;
    line-clamp: 4;
  }
  .stats {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: var(--rt-space-4);
  }
  .stat {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-1);
    padding: var(--rt-space-4) var(--rt-space-5);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-lg);
    background: var(--rt-surface-2);
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .stat .n {
    color: var(--rt-fg);
    font: var(--rt-font-heading);
    font-variant-numeric: tabular-nums;
  }
  .columns {
    display: grid;
    grid-template-columns: 1.2fr 1fr;
    gap: var(--rt-space-7);
  }
  .columns h4:not(:first-child) {
    margin-top: var(--rt-space-6);
  }
  .bars {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .bars li {
    display: grid;
    grid-template-columns: minmax(80px, 160px) 1fr 32px;
    align-items: center;
    gap: var(--rt-space-4);
  }
  .bar-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bar {
    height: 6px;
    overflow: hidden;
    border-radius: var(--rt-radius-pill);
    background: var(--rt-surface-sunken);
  }
  .bar span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--rt-accent-solid);
  }
  .bar-n {
    color: var(--rt-fg-muted);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .auth,
  .servers-list {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .auth li {
    display: flex;
    gap: var(--rt-space-3);
    align-items: flex-start;
  }
  .auth :global(svg) {
    flex: none;
    margin-top: 3px;
    color: var(--rt-fg-muted);
  }
  .servers-list li {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-1);
    overflow-wrap: anywhere;
  }
  .server-name {
    font-weight: 600;
  }

  /* Choose */
  .choose {
    display: grid;
    grid-template-columns: minmax(0, 1.6fr) minmax(240px, 1fr);
    gap: var(--rt-space-6);
    min-height: 0;
  }
  .ops {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
    min-width: 0;
  }
  .ops-tools {
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
  }
  .ops-bar {
    display: flex;
    align-items: center;
    gap: var(--rt-space-2);
  }
  .tree {
    height: min(46vh, 420px);
    overflow: auto;
    padding: var(--rt-space-2);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-lg);
    background: var(--rt-surface-2);
  }
  .group-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: var(--rt-control-md);
    padding: 0 var(--rt-space-3);
    font-weight: 600;
  }
  .op-row {
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
    height: var(--rt-control-md);
    padding: 0 var(--rt-space-3);
    border-radius: var(--rt-radius-md);
    cursor: pointer;
  }
  .op-row.nested {
    padding-left: var(--rt-space-8);
  }
  .op-row:hover {
    background: var(--rt-bg-hover);
  }
  .op-check {
    flex: none;
    width: 14px;
    height: 14px;
    margin: 0;
    accent-color: var(--rt-accent-solid);
  }
  .op-row:has(.op-check:focus-visible) {
    box-shadow: var(--rt-focus-ring);
  }
  .path {
    flex: none;
    max-width: 50%;
    overflow: hidden;
    color: var(--rt-fg);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--rt-fg-muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name.deprecated {
    text-decoration: line-through;
  }
  .settings {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-6);
    min-width: 0;
  }
  .server {
    display: flex;
    align-items: flex-start;
    gap: var(--rt-space-3);
    margin-bottom: var(--rt-space-4);
  }
  .server :global(.rt-checkbox) {
    margin-top: 5px;
  }
  .server-fields {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: var(--rt-space-2);
    min-width: 0;
  }
  .server-url {
    overflow: hidden;
    color: var(--rt-fg-muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .map {
    display: flex;
    align-items: flex-start;
    gap: var(--rt-space-2);
    margin: 0 0 var(--rt-space-3);
    overflow-wrap: anywhere;
  }
  .map :global(svg) {
    flex: none;
    margin-top: 2px;
  }

  /* Target */
  .target {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-6);
  }
  .tiles {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--rt-space-5);
  }

  /* Result */
  .result {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-6);
  }
  .result-head {
    display: flex;
    gap: var(--rt-space-5);
    align-items: flex-start;
  }
  .result-head p {
    margin: var(--rt-space-2) 0 0;
  }
  .ok-icon {
    display: inline-grid;
    flex: none;
    place-items: center;
    width: 40px;
    height: 40px;
    border-radius: var(--rt-radius-pill);
    background: var(--rt-ok-bg);
    color: var(--rt-ok-fg);
  }
  .envs {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-3);
  }
  .envs .caption {
    width: 100%;
    margin: 0;
  }
  .losses details {
    padding: var(--rt-space-4) var(--rt-space-5);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
    background: var(--rt-surface-2);
  }
  .losses details + details {
    margin-top: var(--rt-space-3);
  }
  .losses summary {
    cursor: pointer;
  }
  .losses ul {
    margin-top: var(--rt-space-4);
  }
  .where {
    margin-right: var(--rt-space-2);
    color: var(--rt-fg-muted);
    font-weight: 600;
  }

  @media (max-width: 900px) {
    .choose,
    .columns {
      grid-template-columns: 1fr;
    }
    .stats {
      grid-template-columns: repeat(2, 1fr);
    }
    .drop {
      grid-template-columns: auto 1fr;
    }
    .drop-actions {
      grid-column: 1 / -1;
    }
  }
</style>
