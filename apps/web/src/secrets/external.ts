// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// External secrets in the web app (wave 5, strand E). The browser cannot run `op`, `vault`,
// `aws`, `az` or `gcloud`, so the paired companion reads them (`rt agent --allow-secrets`,
// `POST /v1/secrets`, protocol in `rt_core::transport::companion`). Values stay in memory for
// the session, only to send; they never reach a file, the history or the browser's storage.
// Without a paired companion a send that needs them fails with a hint.

import { BackendError, type ExternalSecrets, type SecretProvider } from "@requesttailor/ui";

import type { CompanionCredentials } from "../wasm";

const LABELS: Record<SecretProvider, string> = {
  "one-password": "1Password",
  "hashicorp-vault-server": "HashiCorp Vault",
  "hashicorp-vault-cloud": "HashiCorp Vault",
  "aws-secrets-manager": "AWS Secrets Manager",
  "azure-key-vault": "Azure Key Vault",
  "gcp-secrets-manager": "Google Cloud Secret Manager",
};

/** A CLI can wait for a login prompt; the companion stops it after 60 s. */
const TIMEOUT_MS = 75_000;

export const NO_COMPANION_FOR_SECRETS =
  "The browser cannot run the secret manager's command line. Run `rt agent --allow-secrets` on this computer, pair it (Send through the companion), then send again.";

export function providerLabel(provider: SecretProvider): string {
  return LABELS[provider] ?? provider;
}

type Fetch = (input: string, init: RequestInit) => Promise<Response>;

export class ExternalSecretReader {
  /** `provider \0 reference \0 name` → value, for this session. */
  private readonly cache = new Map<string, string>();

  constructor(
    private readonly credentials: () => Promise<CompanionCredentials | null>,
    private readonly fetchFn: Fetch = (input, init) => fetch(input, init),
  ) {}

  private key(provider: SecretProvider, reference: string, name: string): string {
    return `${provider}\0${reference}\0${name}`;
  }

  /** Values already read (preview never asks the companion). */
  cached(secrets: ExternalSecrets, skip: string[] = []): [string, string][] {
    const out: [string, string][] = [];
    for (const v of secrets.variables) {
      if (!v.enabled || skip.includes(v.name)) continue;
      const value = this.cache.get(this.key(secrets.provider, v.reference, v.name));
      if (value !== undefined) out.push([v.name, value]);
    }
    return out;
  }

  /** Values of the enabled entries not in `skip`, reading what is not cached yet through the
   *  companion. Rejects with a `BackendError` that says what to do. */
  async resolve(secrets: ExternalSecrets, skip: string[] = []): Promise<[string, string][]> {
    const missing = secrets.variables.filter(
      (v) => v.enabled && !skip.includes(v.name) && !this.cache.has(this.key(secrets.provider, v.reference, v.name)),
    );
    if (missing.length > 0) {
      const label = providerLabel(secrets.provider);
      const companion = await this.credentials();
      if (!companion) {
        throw new BackendError("unsupported", `This environment reads secrets from ${label}. ${NO_COMPANION_FOR_SECRETS}`);
      }
      let res: Response;
      try {
        res = await this.fetchFn(`${companion.url}/v1/secrets`, {
          method: "POST",
          headers: { Authorization: `Bearer ${companion.token}`, "Content-Type": "application/json" },
          body: JSON.stringify({ provider: secrets.provider, variables: missing }),
          cache: "no-store",
          signal: AbortSignal.timeout(TIMEOUT_MS),
        });
      } catch {
        throw new BackendError("unsupported", `The companion did not answer while reading secrets from ${label}. ${NO_COMPANION_FOR_SECRETS}`);
      }
      const body = (await res.json().catch(() => null)) as { values?: unknown; error?: unknown } | null;
      if (res.status === 401) {
        throw new BackendError("unsupported", `The companion is no longer paired. ${NO_COMPANION_FOR_SECRETS}`);
      }
      if (!res.ok) {
        const error = typeof body?.error === "string" ? body.error : `the companion answered ${res.status}`;
        throw new BackendError(res.status === 403 ? "unsupported" : "shell", error);
      }
      const values = Array.isArray(body?.values) ? (body.values as unknown[]) : [];
      for (const v of missing) {
        const pair = values.find((p): p is [string, string] => Array.isArray(p) && p[0] === v.name && typeof p[1] === "string");
        if (!pair) throw new BackendError("shell", `${label}: the companion returned no value for \`${v.name}\`.`);
        this.cache.set(this.key(secrets.provider, v.reference, v.name), pair[1]);
      }
    }
    return this.cached(secrets, skip);
  }

  clear(): void {
    this.cache.clear();
  }
}
