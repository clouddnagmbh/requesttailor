<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The response's "Checks" tab (wave 18, strand B): what the response shows (headers, cookies,
     CORS, versions, stack traces, unsigned tokens), each with where, why and how to fix, and the
     tokens found in the request and the response. Passive: nothing is sent. A rule can be switched
     off for this device or for the collection (an unsaved change in opencollection.yml). -->
<script lang="ts">
  import type { FoundToken, Observation } from "../commands/types";
  import type { ResponseSlotProps } from "../response/slots";
  import { Badge, Button, Icon } from "../ui";
  import { checksVersion, loadChecks, resetChecks } from "./checks.svelte";
  import JwtDialog from "./JwtDialog.svelte";
  import { locationText, sortObservations } from "./model";
  import { owaspShort, owaspTitle } from "../security/owasp";
  import { securityRules } from "../security/rules";

  let { app, requestPath, response }: ResponseSlotProps = $props();

  let observations = $state<Observation[]>([]);
  let tokens = $state<FoundToken[]>([]);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  /** OWASP tags of the security rules (wave 19, strand R), by rule id. */
  let rules = $state(new Map<string, import("../commands/types").SecurityRule>());

  $effect(() => {
    let live = true;
    securityRules(app.backend).then((found) => {
      if (live) rules = found;
    });
    return () => {
      live = false;
    };
  });

  $effect(() => {
    void checksVersion();
    let live = true;
    loaded = false;
    loadChecks(app, requestPath, response).then((found) => {
      if (!live) return;
      observations = sortObservations(found);
      loaded = true;
    });
    app.backend.inspect
      .tokens({ path: requestPath, environment: app.environment, response_id: response.response_id })
      .then((found) => {
        if (live) tokens = found;
      })
      .catch(() => {
        if (live) tokens = [];
      });
    return () => {
      live = false;
    };
  });

  async function switchOff(id: string, scope: "device" | "collection") {
    error = null;
    try {
      await app.backend.inspect.ignoreCheck(id, scope);
      resetChecks();
      if (scope === "collection") await app.refreshDirty();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  function inspect(found: FoundToken) {
    void app.showDialog({
      title: "Token",
      component: JwtDialog,
      width: 640,
      props: { info: found.token, where: locationText(found.location) },
    });
  }
</script>

<div class="checks" data-testid="checks-panel">
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if loaded && observations.length === 0}
    <p class="empty" data-testid="checks-empty">
      Nothing to note. These checks only read the response you already got (headers and the first MiB of the body) and send nothing.
    </p>
  {/if}
  <ul class="list" aria-label="Observations">
    {#each observations as o, i (i)}
      <li class="obs" data-testid="checks-item" data-id={o.id} data-severity={o.severity}>
        <div class="head">
          <Badge tone={o.severity === "warn" ? "warn" : "info"} value={o.severity === "warn" ? "Warning" : "Note"} />
          <code class="id">{o.id}</code>
          {#if rules.get(o.id)}
            {@const rule = rules.get(o.id)!}
            <Badge value={owaspShort(rule.category)} title={owaspTitle(rule.category)} data-testid="checks-owasp" data-category={rule.category} />
          {/if}
          <span class="where">{o.location}</span>
        </div>
        <p class="why">{o.why}</p>
        <p class="fix"><span class="label">Fix:</span> {o.fix}</p>
        <div class="actions">
          <Button size="sm" variant="ghost" data-testid="checks-off-device" onclick={() => switchOff(o.id, "device")}>Turn off on this device</Button>
          <Button size="sm" variant="ghost" data-testid="checks-off-collection" onclick={() => switchOff(o.id, "collection")}>Turn off for this collection</Button>
        </div>
      </li>
    {/each}
  </ul>
  {#if tokens.length > 0}
    <h3>Tokens</h3>
    <ul class="list" aria-label="Tokens">
      {#each tokens as t, i (i)}
        <li class="token" data-testid="checks-token">
          <Icon name="key" size="sm" />
          <span>{locationText(t.location)}</span>
          {#each t.token.warnings as w (w.kind)}<Badge tone="warn" value={w.kind.replace(/-/g, " ")} />{/each}
          <Button size="sm" variant="ghost" data-testid="checks-inspect-token" onclick={() => inspect(t)}>Inspect</Button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .checks {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    padding: var(--rt-space-2) var(--rt-gap);
    font-size: var(--rt-size-small);
  }
  .list {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
  }
  .obs {
    padding-bottom: var(--rt-space-4);
    border-bottom: 1px solid var(--rt-border);
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    flex-wrap: wrap;
  }
  .id {
    font-family: var(--rt-font-mono);
  }
  .where {
    color: var(--rt-fg-muted);
  }
  .why,
  .fix {
    margin: var(--rt-space-2) 0 0;
  }
  .label {
    color: var(--rt-fg-muted);
  }
  .actions {
    display: flex;
    gap: var(--rt-space-2);
    margin-top: var(--rt-space-2);
  }
  .token {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    flex-wrap: wrap;
  }
  h3 {
    margin: var(--rt-space-5) 0 var(--rt-space-3);
    font-size: var(--rt-size-small);
  }
  .empty {
    color: var(--rt-fg-muted);
  }
  .error {
    color: var(--rt-danger-fg);
  }
</style>
