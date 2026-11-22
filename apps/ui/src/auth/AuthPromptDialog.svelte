<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Shown while an OAuth 2.0 flow waits for the user (`backend.auth.onPrompt`): "continue in
     your browser" with the URL, or the device code to enter. Cancel stops the flow.
     Owner: wave 5, strand C. Interface commit: subscribes, shows nothing yet. -->
<script lang="ts">
  import type { AuthPrompt } from "../commands/types";
  import { getApp } from "../state/app.svelte";

  const app = getApp();
  let prompt = $state<AuthPrompt | null>(null);

  $effect(() => app.backend.auth.onPrompt((p) => (prompt = p.kind === "done" ? null : p)));
</script>

{#if prompt}
  <div data-testid="auth-prompt" hidden></div>
{/if}
