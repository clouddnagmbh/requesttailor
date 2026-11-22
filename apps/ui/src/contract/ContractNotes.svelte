<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Breaking changes of the OpenAPI spec between two revisions, as hints (wave 12, strand F).
     Shown by the semantic diff (strand A) and the PR review (strand E); renders nothing when
     there are none. Interface placeholder with the props the other strands rely on: renders
     nothing until the strand fills it in. -->
<script lang="ts">
  let props: { from: string; to: string } = $props();
</script>

<!-- Nothing until strand F; the props are what the other strands pass. -->
<span hidden data-props={JSON.stringify(props)}></span>
