<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Request auth: inherit, none, basic, bearer, API key. Auth the core cannot execute yet
     (`unsupported`) is shown read-only; replacing it is an explicit second step. -->
<script lang="ts">
  import type { ApiKeyPlacement, Auth } from "../commands/types";
  import CodeField from "./CodeField.svelte";
  import type { VarLookup } from "./vars";

  type AuthType = Exclude<Auth["type"], "unsupported">;

  let {
    auth,
    lookup,
    stash,
    onchange,
  }: {
    auth: Auth;
    lookup: VarLookup;
    /** Auth of other types, kept while switching until the request is saved. */
    stash: Map<string, unknown>;
    onchange: (auth: Auth, typing: boolean) => void;
  } = $props();

  const TYPES: [AuthType, string][] = [
    ["inherit", "Inherit"],
    ["none", "No auth"],
    ["basic", "Basic"],
    ["bearer", "Bearer token"],
    ["api-key", "API key"],
  ];
  const FRESH: Record<AuthType, Auth> = {
    inherit: { type: "inherit" },
    none: { type: "none" },
    basic: { type: "basic", username: "", password: "" },
    bearer: { type: "bearer", token: "" },
    "api-key": { type: "api-key", key: "", value: "", placement: "header" },
  };

  let replacing = $state(false);
  const locked = $derived(auth.type === "unsupported" && !replacing);

  function choose(next: AuthType) {
    if (next === auth.type) return;
    stash.set(`auth:${auth.type}`, auth);
    onchange((stash.get(`auth:${next}`) as Auth | undefined) ?? FRESH[next], false);
    replacing = false;
  }
</script>

<div class="auth">
  <label class="type">
    <span>Auth</span>
    <select data-testid="auth-type" value={auth.type} disabled={locked} onchange={(e) => choose(e.currentTarget.value as AuthType)}>
      {#if auth.type === "unsupported"}
        <option value="unsupported">{auth.kind} (not supported)</option>
      {/if}
      {#each TYPES as [value, text] (value)}
        <option {value}>{text}</option>
      {/each}
    </select>
  </label>

  {#if auth.type === "unsupported"}
    <div class="notice" data-testid="auth-unsupported" role="note">
      <p>
        This request uses <strong>{auth.kind}</strong> authentication. RequestTailor cannot edit or send it yet; it stays
        in the file unchanged.
      </p>
      {#if replacing}
        <p class="warn">Choosing another type above removes the {auth.kind} settings from the file when you save.</p>
      {:else}
        <button data-testid="auth-replace" onclick={() => (replacing = true)}>Replace with another type…</button>
      {/if}
    </div>
  {:else if auth.type === "inherit"}
    <p class="hint">Uses the auth of the nearest folder or the collection.</p>
  {:else if auth.type === "none"}
    <p class="hint">No authentication is sent, even if a folder or the collection declares one.</p>
  {:else if auth.type === "basic"}
    <div class="fields">
      <label
        ><span>Username</span>
        <CodeField testid="auth-username" label="Username" value={auth.username} {lookup} onchange={(username) => auth.type === "basic" && onchange({ ...auth, username }, true)} />
      </label>
      <label
        ><span>Password</span>
        <CodeField testid="auth-password" label="Password" value={auth.password} {lookup} onchange={(password) => auth.type === "basic" && onchange({ ...auth, password }, true)} />
      </label>
    </div>
  {:else if auth.type === "bearer"}
    <div class="fields">
      <label
        ><span>Token</span>
        <CodeField testid="auth-token" label="Token" value={auth.token} {lookup} onchange={(token) => auth.type === "bearer" && onchange({ ...auth, token }, true)} />
      </label>
    </div>
  {:else if auth.type === "api-key"}
    <div class="fields">
      <label
        ><span>Key</span>
        <CodeField testid="auth-key" label="Key" value={auth.key} {lookup} onchange={(key) => auth.type === "api-key" && onchange({ ...auth, key }, true)} />
      </label>
      <label
        ><span>Value</span>
        <CodeField testid="auth-value" label="Value" value={auth.value} {lookup} onchange={(value) => auth.type === "api-key" && onchange({ ...auth, value }, true)} />
      </label>
      <label
        ><span>Add to</span>
        <select
          data-testid="auth-placement"
          value={auth.placement}
          onchange={(e) => auth.type === "api-key" && onchange({ ...auth, placement: e.currentTarget.value as ApiKeyPlacement }, false)}
        >
          <option value="header">Header</option>
          <option value="query">Query params</option>
        </select>
      </label>
    </div>
  {/if}
</div>

<style>
  .auth,
  .fields {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
  }
  .fields {
    gap: 4px;
  }
  label {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
  }
  label > span {
    color: var(--rt-fg-muted);
    min-width: 6em;
  }
  .hint {
    color: var(--rt-fg-muted);
    margin: 0;
  }
  .notice {
    border: 1px solid var(--rt-border);
    border-left: 3px solid var(--rt-warn);
    border-radius: var(--rt-radius);
    background: var(--rt-bg-panel);
    padding: 6px 10px;
  }
  .notice p {
    margin: 0 0 6px;
  }
  .warn {
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
  button:hover {
    background: var(--rt-bg-hover);
  }
</style>
