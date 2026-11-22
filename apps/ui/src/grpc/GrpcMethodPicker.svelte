<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Service and method of a gRPC request (wave 8). Interface placeholder: strand B owns it
     (services from proto files or reflection, "Use template"); strand A renders it in the
     session editor of gRPC requests. Props are the contract. -->
<script lang="ts">
  import type { GrpcSpec } from "../commands/types";

  let {
    path,
    spec,
    onchange,
  }: {
    /** Request path; the backend resolves services for it. */
    path: string;
    spec: GrpcSpec;
    /** Called with the changed spec (method, method type, proto file, message template). */
    onchange: (spec: GrpcSpec) => void;
  } = $props();
</script>

<input
  class="method"
  data-testid="grpc-method"
  aria-label="gRPC method"
  data-path={path}
  placeholder="/package.Service/Method"
  value={spec.method}
  onchange={(e) => onchange({ ...spec, method: e.currentTarget.value })}
/>
