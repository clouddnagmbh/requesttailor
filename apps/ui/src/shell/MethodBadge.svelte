<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- HTTP method label, coloured per method. -->
<script lang="ts">
  let { method }: { method: string } = $props();
  const known = ["GET", "POST", "PUT", "PATCH", "DELETE"];
  const kind = $derived(known.includes(method.toUpperCase()) ? method.toLowerCase() : "other");
</script>

<span class="method {kind}" title={method}>{method.toUpperCase()}</span>

<style>
  .method {
    flex: none;
    min-width: 3.2em;
    font: 600 10px / 1 var(--rt-font-mono);
    letter-spacing: 0.02em;
    color: var(--rt-method-other);
  }
  .get {
    color: var(--rt-method-get);
  }
  .post {
    color: var(--rt-method-post);
  }
  .put {
    color: var(--rt-method-put);
  }
  .patch {
    color: var(--rt-method-patch);
  }
  .delete {
    color: var(--rt-method-delete);
  }
</style>
