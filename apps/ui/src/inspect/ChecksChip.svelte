<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "3 checks" in the response's status line (wave 18, strand B): calm, only there when the passive
     checks have something to say. A click lists them. -->
<script lang="ts">
  import type { ResponseSlotProps } from "../response/slots";
  import { Chip } from "../ui";
  import ChecksDialog from "./ChecksDialog.svelte";
  import { checksVersion, loadChecks } from "./checks.svelte";
  import { chipLabel, chipTone } from "./model";
  import type { Observation } from "../commands/types";

  let { app, requestPath, response }: ResponseSlotProps = $props();
  let observations = $state<Observation[]>([]);

  $effect(() => {
    void checksVersion();
    let live = true;
    loadChecks(app, requestPath, response).then((found) => {
      if (live) observations = found;
    });
    return () => {
      live = false;
    };
  });

  function open() {
    void app.showDialog({ title: "Checks", component: ChecksDialog, width: 640, props: { requestPath, response } });
  }
</script>

{#if observations.length > 0}
  <span data-testid="checks-chip" data-tone={chipTone(observations)}>
    <Chip
      label={chipLabel(observations.length)}
      icon={chipTone(observations) === "warn" ? "warning" : "info"}
      tone={chipTone(observations)}
      size="sm"
      title="Passive checks of this response"
      onclick={open}
    />
  </span>
{/if}
