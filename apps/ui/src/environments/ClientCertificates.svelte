<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- TLS client certificates of this environment (domain, PEM or PKCS#12 files, passphrase as a secret variable).
     Owner: wave 5, strand D. Interface commit: a placeholder. -->
<script lang="ts">
  import type { Environment } from "../commands/types";

  let { env, onchange: _onchange }: { env: Environment; onchange: (next: Environment) => void } = $props();
</script>

<div data-testid="client-certs" hidden>{env.name}</div>
