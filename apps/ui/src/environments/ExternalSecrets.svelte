<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Secret values from an external manager (1Password, HashiCorp Vault, AWS Secrets Manager, Azure Key Vault, GCP): provider and one reference per variable.
     Owner: wave 5, strand E. Interface commit: a placeholder. -->
<script lang="ts">
  import type { Environment } from "../commands/types";

  let { env, onchange: _onchange }: { env: Environment; onchange: (next: Environment) => void } = $props();
</script>

<div data-testid="external-secrets" hidden>{env.name}</div>
