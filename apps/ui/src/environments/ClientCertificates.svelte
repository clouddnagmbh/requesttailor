<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- TLS client certificates of this environment (domain, PEM or PKCS#12 files, passphrase as a secret variable).
     Owner: wave 5, strand D. Paths are relative to the collection folder; the files are read only when a
     request to a matching host is sent. Environment entries win over the collection's. -->
<script lang="ts">
  import type { Environment } from "../commands/types";
  import {
    addCertificate,
    certificateProblems,
    removeCertificate,
    setPath,
    setSourceKind,
    updateCertificate,
    type PathField,
    type SourceKind,
  } from "./certs";

  let { env, onchange }: { env: Environment; onchange: (next: Environment) => void } = $props();

  const value = (e: Event) => (e.currentTarget as HTMLInputElement).value;
  const path = (i: number, field: PathField) => (e: Event) => onchange(setPath(env, i, field, value(e)));
</script>

<section class="certs" data-testid="client-certs" aria-labelledby="client-certs-title">
  <h3 id="client-certs-title">Client certificates</h3>
  {#if env.client_certificates.length === 0}
    <p class="hint" data-testid="client-certs-empty">
      No client certificate. Add one for servers that require mutual TLS (mTLS).
    </p>
  {:else}
    <ul>
      {#each env.client_certificates as cert, i (i)}
        {@const problems = certificateProblems(cert)}
        <li data-testid="client-cert" class:disabled={!cert.enabled}>
          <div class="row">
            <input
              data-testid="client-cert-enabled"
              type="checkbox"
              aria-label="Enabled"
              checked={cert.enabled}
              onchange={(e) => onchange(updateCertificate(env, i, { enabled: e.currentTarget.checked }))}
            />
            <label class="grow"
              ><span>Host</span>
              <input
                data-testid="client-cert-domain"
                aria-label="Host"
                placeholder="api.example.com, *.example.com or localhost:8443"
                value={cert.domain}
                onchange={(e) => onchange(updateCertificate(env, i, { domain: value(e) }))}
              />
            </label>
            <select
              data-testid="client-cert-type"
              aria-label="Certificate format"
              value={cert.source.type}
              onchange={(e) => onchange(setSourceKind(env, i, e.currentTarget.value as SourceKind))}
            >
              <option value="pem">PEM</option>
              <option value="pkcs12">PKCS#12</option>
            </select>
            <button data-testid="client-cert-remove" aria-label="Remove certificate" onclick={() => onchange(removeCertificate(env, i))}>
              ×
            </button>
          </div>
          {#if cert.source.type === "pem"}
            <label
              ><span>Certificate</span>
              <input
                data-testid="client-cert-cert-path"
                aria-label="Certificate file"
                placeholder="certs/client.pem"
                value={cert.source.certificate_path}
                onchange={path(i, "certificate_path")}
              />
            </label>
            <label
              ><span>Private key</span>
              <input
                data-testid="client-cert-key-path"
                aria-label="Private key file"
                placeholder="certs/client.key"
                value={cert.source.private_key_path}
                onchange={path(i, "private_key_path")}
              />
            </label>
          {:else}
            <label
              ><span>Archive</span>
              <input
                data-testid="client-cert-p12-path"
                aria-label="PKCS#12 file"
                placeholder="certs/client.p12"
                value={cert.source.path}
                onchange={path(i, "path")}
              />
            </label>
          {/if}
          <label
            ><span>Passphrase</span>
            <input
              data-testid="client-cert-passphrase"
              aria-label="Passphrase"
              placeholder={"{{certPassphrase}}"}
              value={cert.passphrase}
              onchange={(e) => onchange(updateCertificate(env, i, { passphrase: value(e) }))}
            />
          </label>
          {#each problems as problem (problem)}
            <p class="warn" data-testid="client-cert-problem">{problem}</p>
          {/each}
        </li>
      {/each}
    </ul>
  {/if}
  <button data-testid="client-cert-add" onclick={() => onchange(addCertificate(env))}>Add certificate</button>
  <p class="hint">
    Paths are relative to the collection folder. Put the passphrase in a secret variable, e.g.
    <code>{"{{certPassphrase}}"}</code>, so it stays out of the file.
  </p>
</section>

<style>
  .certs {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    margin-top: var(--rt-gap);
  }
  h3 {
    margin: 0;
    font-size: inherit;
  }
  ul {
    align-self: stretch;
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
  }
  li {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: var(--rt-gap);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
  }
  li.disabled {
    opacity: 0.7;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
  }
  label {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
  }
  .grow {
    flex: 1;
  }
  label > span {
    color: var(--rt-fg-muted);
    min-width: 6em;
  }
  label input {
    flex: 1;
  }
  input:not([type="checkbox"]),
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
  .hint {
    color: var(--rt-fg-muted);
    margin: 0;
  }
  .warn {
    color: var(--rt-warn);
    margin: 0;
  }
</style>
