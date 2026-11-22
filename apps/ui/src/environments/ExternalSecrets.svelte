<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Secret values from an external manager (1Password, HashiCorp Vault, AWS Secrets Manager, Azure Key Vault, GCP): provider and one reference per variable.
     Owner: wave 5, strand E. The files hold only the references; there is no value field.
     The shells read the values with the manager's command line when sending (the web app
     through the companion, `rt agent --allow-secrets`) and keep them in memory. -->
<script lang="ts">
  import type { Environment, ExternalSecret, ExternalSecrets, SecretProvider } from "../commands/types";

  let { env, onchange }: { env: Environment; onchange: (next: Environment) => void } = $props();

  const PROVIDERS: { id: SecretProvider; label: string; placeholder: string; help: string }[] = [
    {
      id: "one-password",
      label: "1Password",
      placeholder: "op://vault/item/field",
      help: "A secret reference (op://vault/item/field), read with the 1Password CLI (op).",
    },
    {
      id: "hashicorp-vault-server",
      label: "HashiCorp Vault (server)",
      placeholder: "secret/app#field",
      help: "The secret's path and #field (defaults to the variable name), read with the vault CLI.",
    },
    {
      id: "hashicorp-vault-cloud",
      label: "HashiCorp Vault (HCP)",
      placeholder: "secret/app#field",
      help: "The secret's path and #field (defaults to the variable name), read with the vault CLI.",
    },
    {
      id: "aws-secrets-manager",
      label: "AWS Secrets Manager",
      placeholder: "secret-name#key",
      help: "The secret's name or ARN, #key for one key of a JSON secret, read with the aws CLI.",
    },
    {
      id: "azure-key-vault",
      label: "Azure Key Vault",
      placeholder: "vault-name/secret-name",
      help: "vault-name/secret-name[/version] (the secret defaults to the variable name), read with the az CLI.",
    },
    {
      id: "gcp-secrets-manager",
      label: "Google Cloud Secret Manager",
      placeholder: "project/secret",
      help: "project/secret[/version] (version defaults to latest), read with the gcloud CLI.",
    },
  ];

  const current = $derived(env.external_secrets);
  const provider = $derived(PROVIDERS.find((p) => p.id === current?.provider) ?? PROVIDERS[0]);

  function emit(external: ExternalSecrets | null): void {
    const next = $state.snapshot(env) as Environment;
    onchange({ ...next, external_secrets: external });
  }

  function variables(): ExternalSecret[] {
    return current ? ($state.snapshot(current.variables) as ExternalSecret[]) : [];
  }

  function setProvider(value: string): void {
    if (value === "") {
      emit(null);
      return;
    }
    emit({ provider: value as SecretProvider, variables: variables() });
  }

  function setRow(i: number, patch: Partial<ExternalSecret>): void {
    if (!current) return;
    const rows = variables();
    rows[i] = { ...rows[i], ...patch };
    emit({ provider: current.provider, variables: rows });
  }

  function addRow(): void {
    if (!current) return;
    emit({ provider: current.provider, variables: [...variables(), { name: "", reference: "", enabled: true }] });
  }

  function removeRow(i: number): void {
    if (!current) return;
    emit({ provider: current.provider, variables: variables().filter((_, j) => j !== i) });
  }
</script>

<section class="external" data-testid="external-secrets" aria-labelledby="external-secrets-title">
  <div class="head">
    <h3 id="external-secrets-title">External secrets</h3>
    <label>
      <span class="visually-hidden">Secret manager</span>
      <select
        data-testid="external-provider"
        aria-label="Secret manager"
        value={current?.provider ?? ""}
        onchange={(e) => setProvider(e.currentTarget.value)}
      >
        <option value="">None</option>
        {#each PROVIDERS as p (p.id)}
          <option value={p.id}>{p.label}</option>
        {/each}
      </select>
    </label>
  </div>

  {#if current}
    <p class="hint" data-testid="external-help">{provider.help}</p>
    {#if current.variables.length > 0}
      <table>
        <thead>
          <tr>
            <th>Enabled</th>
            <th>Variable</th>
            <th>Reference</th>
            <th><span class="visually-hidden">Actions</span></th>
          </tr>
        </thead>
        <tbody>
          {#each current.variables as v, i (i)}
            <tr data-testid="external-var">
              <td>
                <input
                  data-testid="external-enabled"
                  type="checkbox"
                  aria-label="Enabled"
                  checked={v.enabled}
                  onchange={(e) => setRow(i, { enabled: e.currentTarget.checked })}
                />
              </td>
              <td>
                <input
                  data-testid="external-name"
                  aria-label="Variable name"
                  value={v.name}
                  onchange={(e) => setRow(i, { name: e.currentTarget.value.trim() })}
                />
              </td>
              <td class="reference">
                <input
                  data-testid="external-reference"
                  aria-label="Reference"
                  autocomplete="off"
                  spellcheck="false"
                  placeholder={provider.placeholder}
                  value={v.reference}
                  onchange={(e) => setRow(i, { reference: e.currentTarget.value.trim() })}
                />
              </td>
              <td>
                <button data-testid="external-remove" aria-label="Remove external secret" onclick={() => removeRow(i)}>
                  ×
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
    <button data-testid="external-add" onclick={addRow}>Add external secret</button>
    <p class="hint">
      Only the reference is saved. The value is read with your own login when you send, kept in memory for
      this session and never written to a file. In the browser this needs the companion:
      <code>rt agent --allow-secrets</code>.
    </p>
  {/if}
</section>

<style>
  .external {
    margin-top: calc(var(--rt-gap) * 2);
    padding-top: var(--rt-gap);
    border-top: 1px solid var(--rt-border);
  }

  .head {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
  }

  h3 {
    flex: 1;
    margin: 0;
    font-size: inherit;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    margin-bottom: var(--rt-gap);
  }

  th {
    text-align: left;
    font-weight: normal;
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }

  td {
    padding: 2px 4px;
  }

  td.reference {
    width: 55%;
  }

  td input:not([type="checkbox"]) {
    width: 100%;
    box-sizing: border-box;
  }

  .reference input {
    font-family: var(--rt-font-mono, monospace);
  }

  .hint {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
</style>
