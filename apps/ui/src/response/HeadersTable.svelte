<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Response headers, and the request headers as sent (secrets already masked). -->
<script lang="ts">
  let {
    response,
    request,
  }: { response: [string, string][]; request: [string, string][] | null } = $props();
</script>

<div class="headers">
  <h3>Response headers <span class="count">{response.length}</span></h3>
  <table data-testid="response-headers">
    <tbody>
      {#each response as [name, value], i (i)}
        <tr><th scope="row">{name}</th><td>{value}</td></tr>
      {:else}
        <tr><td class="none" colspan="2">No headers</td></tr>
      {/each}
    </tbody>
  </table>
  {#if request}
    <h3>Request headers <span class="count">{request.length}</span></h3>
    <table data-testid="request-headers">
      <tbody>
        {#each request as [name, value], i (i)}
          <tr><th scope="row">{name}</th><td>{value}</td></tr>
        {:else}
          <tr><td class="none" colspan="2">No headers</td></tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<style>
  .headers {
    overflow: auto;
    height: 100%;
    padding: var(--rt-gap);
    box-sizing: border-box;
  }
  h3 {
    margin: 0 0 4px;
    font-size: var(--rt-size-small);
    font-weight: 600;
    color: var(--rt-fg-muted);
  }
  h3:not(:first-child) {
    margin-top: calc(2 * var(--rt-gap));
  }
  .count {
    font-weight: normal;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font: var(--rt-size-small) / 1.4 var(--rt-font-mono);
  }
  th,
  td {
    padding: 2px var(--rt-gap);
    border-bottom: 1px solid var(--rt-border);
    vertical-align: top;
    text-align: left;
    word-break: break-all;
  }
  th {
    width: 30%;
    font-weight: normal;
    color: var(--rt-fg-muted);
  }
  .none {
    color: var(--rt-fg-muted);
  }
</style>
