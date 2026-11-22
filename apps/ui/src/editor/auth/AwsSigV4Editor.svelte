<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- AWS Signature V4 settings of a request (wave 5, strand D): access key, secret key,
     session token, service, region, profile. Interface commit: a placeholder. -->
<script lang="ts">
  import type { Auth } from "../../commands/types";
  import type { VarLookup } from "../vars";

  let {
    auth,
    lookup: _lookup,
    onchange: _onchange,
  }: {
    auth: Extract<Auth, { type: "awsv4" }>;
    lookup: VarLookup;
    onchange: (auth: Auth, typing: boolean) => void;
  } = $props();
</script>

<p class="hint" data-testid="auth-awsv4">AWS Signature V4 ({auth.service || "no service"}, {auth.region || "no region"})</p>
