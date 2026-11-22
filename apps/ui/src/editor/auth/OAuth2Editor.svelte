<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- OAuth 2.0 settings of a request (wave 5, strand C): flow, endpoints, client, scope, extra
     parameters, token placement and the automatic fetch/refresh switches, plus "Get token"
     (`backend.auth.fetchToken`) with the status of the token it got and the token overview.
     Token values never reach the UI. Edit logic lives in `./oauth2.ts`. -->
<script lang="ts">
  import TokensDialog from "../../auth/TokensDialog.svelte";
  import { tokenState, when } from "../../auth/format";
  import type { Auth, CredentialsPlacement, OAuth2Param, PkceMethod, TokenInfo, TokenSource } from "../../commands/types";
  import { getApp } from "../../state/app.svelte";
  import CodeField from "../CodeField.svelte";
  import type { VarLookup } from "../vars";
  import {
    COLLECTION_FLOWS,
    REQUEST_FLOWS,
    collectionOnly,
    stepsFor,
    switchFlow,
    switchTokenPlacement,
    type FlowName,
    type OAuth2Auth,
  } from "./oauth2";
  import OAuth2Params from "./OAuth2Params.svelte";

  let {
    auth,
    lookup,
    onchange,
  }: {
    auth: OAuth2Auth;
    lookup: VarLookup;
    onchange: (auth: Auth, typing: boolean) => void;
  } = $props();

  const app = getApp();

  /** Shallow update of the current auth. Flow fields are only set where the flow has them. */
  function set(patch: Record<string, unknown>, typing = true) {
    onchange({ ...auth, ...patch } as OAuth2Auth, typing);
  }

  const browserFlow = $derived(auth.flow === "authorization-code" || auth.flow === "implicit");
  const tokenEndpoint = $derived(auth.flow !== "implicit");
  const flowLabel = (flow: FlowName) => [...REQUEST_FLOWS, ...COLLECTION_FLOWS].find(([f]) => f === flow)?.[1] ?? flow;

  // ---- token ----

  let fetching = $state(false);
  let token = $state<TokenInfo | null>(null);
  let tokenError = $state<string | null>(null);
  let showingTokens = $state(false);
  /** The request the status belongs to; a different request starts without one. */
  let statusFor = $state<string | null>(null);

  $effect(() => {
    if (app.active !== statusFor) {
      token = null;
      tokenError = null;
    }
  });

  async function getToken() {
    const path = app.active;
    if (!path || fetching) return;
    fetching = true;
    token = null;
    tokenError = null;
    statusFor = path;
    try {
      // Hand pending typing to the backend first and wait for it, so the flow runs with what
      // the editor shows.
      onchange(auth, false);
      const draft = app.drafts[path];
      if (draft) await app.edit(path, $state.snapshot(draft));
      token = await app.backend.auth.fetchToken(path, { environment: app.environment, vars: [] });
    } catch (e) {
      tokenError = e instanceof Error ? e.message : String(e);
    } finally {
      fetching = false;
    }
  }
</script>

<div class="oauth2" data-testid="auth-oauth2">
  <div class="fields">
    <label
      ><span>Flow</span>
      <select
        data-testid="oauth2-flow"
        value={auth.flow}
        onchange={(e) => onchange(switchFlow(auth, e.currentTarget.value as FlowName), false)}
      >
        {#each REQUEST_FLOWS as [value, text] (value)}
          <option {value}>{text}</option>
        {/each}
        {#if collectionOnly(auth.flow)}
          <option value={auth.flow}>{flowLabel(auth.flow)} (collection default only)</option>
        {/if}
      </select>
    </label>
  </div>

  {#if auth.flow === "implicit"}
    <p class="notice" role="note" data-testid="oauth2-implicit-note">
      The implicit flow is kept in the file but not executed: OAuth 2.1 removes it because the token travels in the
      browser URL. Use <strong>Authorization code</strong> with PKCE instead.
    </p>
  {:else if collectionOnly(auth.flow)}
    <p class="notice" role="note" data-testid="oauth2-collection-only-note">
      {flowLabel(auth.flow)} has no OpenCollection form for requests. It can only be the collection's default auth (in
      <code>extensions.requesttailor.auth</code> of <code>opencollection.yml</code>); a request with it cannot be saved.
      Choose another flow, or set the request's auth to Inherit.
    </p>
  {/if}

  <fieldset>
    <legend>Endpoints</legend>
    <div class="fields">
      {#if auth.flow === "authorization-code" || auth.flow === "implicit"}
        <label
          ><span>Authorization URL</span>
          <CodeField testid="oauth2-authorization-url" label="Authorization URL" placeholder="https://idp.example/authorize" value={auth.authorization_url} {lookup} onchange={(authorization_url) => set({ authorization_url })} />
        </label>
        <label
          ><span>Callback URL</span>
          <CodeField testid="oauth2-callback-url" label="Callback URL" placeholder="http://127.0.0.1:8765/callback" value={auth.callback_url} {lookup} onchange={(callback_url) => set({ callback_url })} />
        </label>
      {/if}
      {#if auth.flow === "device-code"}
        <label
          ><span>Device authorization URL</span>
          <CodeField testid="oauth2-device-url" label="Device authorization URL" value={auth.device_authorization_url} {lookup} onchange={(device_authorization_url) => set({ device_authorization_url })} />
        </label>
      {/if}
      {#if tokenEndpoint}
        <label
          ><span>Token URL</span>
          <CodeField testid="oauth2-token-url" label="Token URL" placeholder="https://idp.example/token" value={auth.access_token_url} {lookup} onchange={(access_token_url) => set({ access_token_url })} />
        </label>
        <label
          ><span>Refresh URL</span>
          <CodeField testid="oauth2-refresh-url" label="Refresh URL" placeholder="same as the token URL" value={auth.refresh_token_url} {lookup} onchange={(refresh_token_url) => set({ refresh_token_url })} />
        </label>
      {/if}
    </div>
  </fieldset>

  <fieldset>
    <legend>Client</legend>
    <div class="fields">
      <label
        ><span>Client ID</span>
        <CodeField testid="oauth2-client-id" label="Client ID" value={auth.client_id} {lookup} onchange={(client_id) => set({ client_id })} />
      </label>
      {#if tokenEndpoint}
        <label
          ><span>Client secret</span>
          <CodeField testid="oauth2-client-secret" label="Client secret" placeholder={"{{clientSecret}} (a secret variable)"} value={auth.client_secret} {lookup} onchange={(client_secret) => set({ client_secret })} />
        </label>
        <label
          ><span>Send client as</span>
          <select
            data-testid="oauth2-credentials-placement"
            value={auth.credentials_placement}
            onchange={(e) => set({ credentials_placement: e.currentTarget.value as CredentialsPlacement }, false)}
          >
            <option value="basic-auth-header">Basic auth header</option>
            <option value="body">Body fields</option>
          </select>
        </label>
      {/if}
      {#if auth.flow === "password"}
        <label
          ><span>Username</span>
          <CodeField testid="oauth2-username" label="Username" value={auth.username} {lookup} onchange={(username) => set({ username })} />
        </label>
        <label
          ><span>Password</span>
          <CodeField testid="oauth2-password" label="Password" placeholder={"{{password}} (a secret variable)"} value={auth.password} {lookup} onchange={(password) => set({ password })} />
        </label>
      {/if}
      {#if auth.flow === "saml2-bearer"}
        <label
          ><span>SAML assertion</span>
          <CodeField testid="oauth2-assertion" label="SAML assertion (base64url)" placeholder={"{{samlAssertion}}"} value={auth.assertion} {lookup} onchange={(assertion) => set({ assertion })} />
        </label>
      {/if}
      <label
        ><span>Scope</span>
        <CodeField testid="oauth2-scope" label="Scope" placeholder="space-separated, e.g. openid profile" value={auth.scope} {lookup} onchange={(scope) => set({ scope })} />
      </label>
      {#if browserFlow && (auth.flow === "authorization-code" || auth.flow === "implicit")}
        <label
          ><span>State</span>
          <CodeField testid="oauth2-state" label="State" placeholder="random for every sign-in" value={auth.state} {lookup} onchange={(state) => set({ state })} />
        </label>
      {/if}
      {#if auth.flow === "authorization-code"}
        <label
          ><span>PKCE</span>
          <select
            data-testid="oauth2-pkce"
            value={auth.pkce ?? "off"}
            onchange={(e) => {
              const v = e.currentTarget.value;
              set({ pkce: v === "off" ? null : (v as PkceMethod) }, false);
            }}
          >
            <option value="S256">S256 (recommended)</option>
            <option value="plain">plain</option>
            <option value="off">Off</option>
          </select>
        </label>
      {/if}
    </div>
  </fieldset>

  <fieldset>
    <legend>Extra parameters</legend>
    <OAuth2Params
      params={auth.parameters}
      steps={stepsFor(auth.flow)}
      {lookup}
      onchange={(parameters: OAuth2Param[], typing: boolean) => set({ parameters }, typing)}
    />
  </fieldset>

  <fieldset>
    <legend>Token</legend>
    <div class="fields">
      <label
        ><span>Use</span>
        <select data-testid="oauth2-token-source" value={auth.token.source} onchange={(e) => set({ token: { ...auth.token, source: e.currentTarget.value as TokenSource } }, false)}>
          <option value="access-token">Access token</option>
          <option value="id-token">ID token</option>
        </select>
      </label>
      <label
        ><span>Add to</span>
        <select
          data-testid="oauth2-token-placement"
          value={auth.token.placement.in}
          onchange={(e) => set({ token: switchTokenPlacement(auth.token, e.currentTarget.value as "header" | "query") }, false)}
        >
          <option value="header">Authorization header</option>
          <option value="query">Query parameter</option>
        </select>
      </label>
      {#if auth.token.placement.in === "header"}
        <label
          ><span>Prefix</span>
          <CodeField testid="oauth2-token-prefix" label="Header prefix" placeholder="none" value={auth.token.placement.prefix} {lookup} onchange={(prefix) => set({ token: { ...auth.token, placement: { in: "header", prefix } } })} />
        </label>
      {:else}
        <label
          ><span>Parameter</span>
          <CodeField testid="oauth2-token-query" label="Query parameter name" value={auth.token.placement.name} {lookup} onchange={(name) => set({ token: { ...auth.token, placement: { in: "query", name } } })} />
        </label>
      {/if}
      <label
        ><span>Token ID</span>
        <CodeField testid="oauth2-token-id" label="Token ID" placeholder="share one token between requests with the same ID" value={auth.token.id} {lookup} onchange={(id) => set({ token: { ...auth.token, id } })} />
      </label>
      <label class="check"
        ><input type="checkbox" data-testid="oauth2-auto-fetch" checked={auth.auto_fetch} onchange={(e) => set({ auto_fetch: e.currentTarget.checked }, false)} />
        Fetch a token automatically when none is cached</label
      >
      <label class="check"
        ><input type="checkbox" data-testid="oauth2-auto-refresh" checked={auth.auto_refresh} onchange={(e) => set({ auto_refresh: e.currentTarget.checked }, false)} />
        Refresh an expired token automatically</label
      >
    </div>
  </fieldset>

  <div class="token-actions">
    <button data-testid="oauth2-get-token" disabled={fetching || auth.flow === "implicit" || !app.active} onclick={getToken}>
      {fetching ? "Getting token…" : "Get new token"}
    </button>
    <button data-testid="oauth2-tokens" onclick={() => (showingTokens = true)}>Tokens…</button>
    <span class="status" aria-live="polite" data-testid="oauth2-token-status">
      {#if tokenError}
        <span class="error">{tokenError}</span>
      {:else if token}
        {@const status = tokenState(token, Date.now())}
        <span class={status.kind}>Token received {when(token.obtained_at)}, {status.text}{token.refreshable ? ", refreshable" : ""}.</span>
      {/if}
    </span>
  </div>
  <p class="hint">Tokens are cached like secrets, outside the collection files, and are never shown.</p>
</div>

<TokensDialog open={showingTokens} onclose={() => (showingTokens = false)} />

<style>
  .oauth2,
  .fields {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
  }
  .fields {
    gap: 4px;
  }
  fieldset {
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    margin: 0;
    padding: 4px 8px 8px;
  }
  legend {
    color: var(--rt-fg-muted);
    padding: 0 4px;
  }
  label {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
  }
  label > span {
    color: var(--rt-fg-muted);
    min-width: 10em;
  }
  label.check {
    gap: 6px;
  }
  .notice {
    margin: 0;
    border: 1px solid var(--rt-border);
    border-left: 3px solid var(--rt-warn);
    border-radius: var(--rt-radius);
    background: var(--rt-bg-panel);
    padding: 6px 10px;
  }
  .hint {
    color: var(--rt-fg-muted);
    margin: 0;
  }
  .token-actions {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
    flex-wrap: wrap;
  }
  .error {
    color: var(--rt-danger);
  }
  .valid {
    color: var(--rt-ok);
  }
  .expired {
    color: var(--rt-warn);
  }
  select,
  button {
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px 6px;
  }
  button {
    cursor: pointer;
    background: var(--rt-bg-panel);
  }
  button:hover:not(:disabled) {
    background: var(--rt-bg-hover);
  }
</style>
