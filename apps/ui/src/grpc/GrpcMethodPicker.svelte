<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Service and method of a gRPC request (wave 8, strand B). Services come from
     `Backend.grpc.services`: the request's proto file, the collection's `config.protobuf`, or
     server reflection. Picking a method sets its type; "Use template" fills the selected
     message with a JSON skeleton of the input message. Strand A renders it in the session
     editor of gRPC requests. Props are the contract. -->
<script lang="ts">
  import { onMount } from "svelte";

  import type { GrpcService, GrpcSpec } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { chooseMethod, findMethod, METHOD_TYPE_LABEL, withTemplate } from "./picker";

  let {
    path,
    spec,
    onchange,
  }: {
    /** Request path; the backend resolves services for it. */
    path: string;
    spec: GrpcSpec;
    /** Called with the changed spec (method, method type, proto file, message template). */
    onchange: (spec: GrpcSpec) => void;
  } = $props();

  const app = getApp();

  let services = $state<GrpcService[] | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  const input = () => ({ environment: app.environment, vars: [] as [string, string][] });
  const known = $derived(services ? findMethod(services, spec.method) : undefined);

  async function load() {
    loading = true;
    error = null;
    try {
      services = await app.backend.grpc.services(path, input());
    } catch (e) {
      services = null;
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function useTemplate() {
    error = null;
    try {
      const template = await app.backend.grpc.template(path, spec.method, input());
      onchange(withTemplate(spec, template));
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  function pick(method: string) {
    if (!services || method === "") return;
    onchange(chooseMethod(spec, services, method));
  }

  onMount(() => {
    void load();
  });
</script>

<div class="picker" data-testid="grpc-picker">
  <div class="row">
    <select
      data-testid="grpc-method-select"
      aria-label="gRPC service and method"
      disabled={!services || services.length === 0}
      value={known ? spec.method : ""}
      onchange={(e) => pick(e.currentTarget.value)}
    >
      <option value="" disabled>{loading ? "Loading methods…" : services ? "Choose a method" : "No methods"}</option>
      {#each services ?? [] as service (service.name)}
        <optgroup label={service.name}>
          {#each service.methods as m (m.path)}
            <option value={m.path}>{m.name} · {METHOD_TYPE_LABEL[m.method_type]}</option>
          {/each}
        </optgroup>
      {/each}
    </select>
    <button type="button" data-testid="grpc-reload" onclick={load} disabled={loading} title="Read the proto files or ask the server again">
      Reload
    </button>
    <button type="button" data-testid="grpc-use-template" onclick={useTemplate} disabled={!known}>Use template</button>
  </div>
  <div class="row">
    <input
      class="method"
      data-testid="grpc-method"
      aria-label="gRPC method"
      data-path={path}
      placeholder="/package.Service/Method"
      value={spec.method}
      onchange={(e) => onchange(services ? chooseMethod(spec, services, e.currentTarget.value) : { ...spec, method: e.currentTarget.value })}
    />
    <span class="type" data-testid="grpc-method-type">{METHOD_TYPE_LABEL[spec.method_type]}</span>
  </div>
  {#if known}
    <p class="hint" data-testid="grpc-method-io">{known.input_type} → {known.output_type}</p>
  {/if}
  {#if error}
    <p class="error" role="alert" data-testid="grpc-error">{error}</p>
  {/if}
</div>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
  }
  .row {
    display: flex;
    gap: var(--rt-gap);
    align-items: center;
    min-width: 0;
  }
  select,
  .method {
    flex: 1;
    min-width: 0;
    font: inherit;
  }
  .method {
    font-family: var(--rt-font-mono);
  }
  .type,
  .hint {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
    margin: 0;
    white-space: nowrap;
  }
  .hint {
    font-family: var(--rt-font-mono);
    white-space: normal;
    overflow-wrap: anywhere;
  }
  .error {
    color: var(--rt-danger);
    margin: 0;
    overflow-wrap: anywhere;
  }
</style>
