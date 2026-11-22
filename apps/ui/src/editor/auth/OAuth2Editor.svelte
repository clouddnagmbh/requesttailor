<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- OAuth 2.0 settings of a request (wave 5, strand C): flow, endpoints, client, scope, extra
     parameters, token placement, and the cached token with "Get new token". Interface
     commit: a placeholder. -->
<script lang="ts">
  import type { Auth } from "../../commands/types";
  import type { VarLookup } from "../vars";

  let {
    auth,
    lookup: _lookup,
    onchange: _onchange,
  }: {
    auth: Extract<Auth, { type: "oauth2" }>;
    lookup: VarLookup;
    onchange: (auth: Auth, typing: boolean) => void;
  } = $props();
</script>

<p class="hint" data-testid="auth-oauth2">OAuth 2.0 ({auth.flow})</p>
