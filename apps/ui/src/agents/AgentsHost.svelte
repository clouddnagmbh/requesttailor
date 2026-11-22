<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Everything of coding agents in the shell (wave 8, strand C): the approval banner and the
     policy and audit dialogs, opened from the palette (`agentCommands` in ./ui.svelte.ts). -->
<script lang="ts">
  import ApprovalBanner from "./ApprovalBanner.svelte";
  import AuditLog from "./AuditLog.svelte";
  import PolicyEditor from "./PolicyEditor.svelte";
  import { agentsUi } from "./ui.svelte";
</script>

<ApprovalBanner />
<PolicyEditor
  open={agentsUi.policy}
  onclose={() => (agentsUi.policy = false)}
  onaudit={() => {
    agentsUi.policy = false;
    agentsUi.audit = true;
  }}
/>
<AuditLog open={agentsUi.audit} onclose={() => (agentsUi.audit = false)} />
