<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- AWS Signature V4 settings of a request (wave 5, strand D): access key, secret key,
     session token, service, region, profile. Every field is a template; keys belong in secret
     variables. The core signs the final request just before it goes on the wire. -->
<script lang="ts">
  import type { Auth } from "../../commands/types";
  import CodeField from "../CodeField.svelte";
  import type { VarLookup } from "../vars";

  type AwsSigV4 = Extract<Auth, { type: "awsv4" }>;
  type Field = Exclude<keyof AwsSigV4, "type">;

  let {
    auth,
    lookup,
    onchange,
  }: {
    auth: AwsSigV4;
    lookup: VarLookup;
    onchange: (auth: Auth, typing: boolean) => void;
  } = $props();

  const FIELDS: { key: Field; label: string; testid: string; placeholder: string }[] = [
    { key: "access_key_id", label: "Access key ID", testid: "awsv4-access-key-id", placeholder: "{{awsAccessKeyId}}" },
    {
      key: "secret_access_key",
      label: "Secret access key",
      testid: "awsv4-secret-access-key",
      placeholder: "{{awsSecretAccessKey}}",
    },
    { key: "session_token", label: "Session token", testid: "awsv4-session-token", placeholder: "optional" },
    { key: "service", label: "Service", testid: "awsv4-service", placeholder: "e.g. execute-api, s3" },
    { key: "region", label: "Region", testid: "awsv4-region", placeholder: "e.g. eu-central-1" },
    { key: "profile_name", label: "Profile", testid: "awsv4-profile-name", placeholder: "optional" },
  ];

  /** The secret key and session token are credentials: typed in plain text they would land in the file. */
  const literalSecret = $derived(
    [auth.secret_access_key, auth.session_token].some((v) => v.trim() !== "" && !/^\s*\{\{[^}]+\}\}\s*$/.test(v)),
  );
  const unsignable = $derived(
    auth.profile_name.trim() !== "" && (auth.access_key_id.trim() === "" || auth.secret_access_key.trim() === ""),
  );

  function set(key: Field, value: string) {
    onchange({ ...auth, [key]: value }, true);
  }
</script>

<div class="fields" data-testid="auth-awsv4">
  {#each FIELDS as f (f.key)}
    <label
      ><span>{f.label}</span>
      <CodeField
        testid={f.testid}
        label={f.label}
        value={auth[f.key]}
        placeholder={f.placeholder}
        {lookup}
        onchange={(v) => set(f.key, v)}
      />
    </label>
  {/each}
  {#if literalSecret}
    <p class="warn" data-testid="awsv4-literal-secret">
      The secret key or session token is written into the request file as typed. Use a secret variable such as
      <code>{"{{awsSecretAccessKey}}"}</code> instead.
    </p>
  {/if}
  {#if unsignable}
    <p class="warn" data-testid="awsv4-profile-hint">
      The profile is only kept in the file; RequestTailor does not read AWS profiles. Without an access key ID and a
      secret access key the request cannot be signed, and sending fails with a message.
    </p>
  {:else}
    <p class="hint">
      The profile is only kept in the file. Signing needs the access key ID and secret access key; the secret key and
      session token are masked in the response report.
    </p>
  {/if}
</div>

<style>
  .fields {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  label {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
  }
  label > span {
    color: var(--rt-fg-muted);
    min-width: 9em;
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
