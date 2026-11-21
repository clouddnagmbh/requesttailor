<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Timing waterfall: one bar per phase on a common time axis. Phases the transport cannot
     see are left out and listed with the reason. -->
<script lang="ts">
  import { formatDuration, type Waterfall } from "./format";

  let { data }: { data: Waterfall } = $props();

  const pct = (ms: number) => (data.total > 0 ? (ms / data.total) * 100 : 0);
</script>

<div class="timing" data-testid="response-timing">
  <table>
    <tbody>
      {#each data.phases as p (p.name)}
        <tr data-phase={p.name}>
          <th scope="row">{p.label}</th>
          <td class="axis">
            <span
              class="bar {p.name}"
              style:left="{pct(p.start)}%"
              style:width="max(2px, {pct(p.duration)}%)"
              title="{p.label}: {formatDuration(p.duration)} (from {formatDuration(p.start)})"
            ></span>
          </td>
          <td class="value">{formatDuration(p.name === "ttfb" ? p.start + p.duration : p.duration)}</td>
        </tr>
      {/each}
      <tr class="total">
        <th scope="row">Total</th>
        <td class="axis"></td>
        <td class="value">{formatDuration(data.total)}</td>
      </tr>
    </tbody>
  </table>
  {#if data.hidden.length > 0}
    <p class="hidden" data-testid="response-timing-hidden">
      Not shown:
      {#each data.hidden as h, i (h.name)}{i > 0 ? "; " : ""}{h.label} ({h.reason}){/each}.
    </p>
  {/if}
  <p class="note">Waiting (TTFB) is the time from sending to the first response byte.</p>
</div>

<style>
  .timing {
    padding: var(--rt-gap);
    overflow: auto;
    height: 100%;
    box-sizing: border-box;
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th {
    width: 120px;
    padding: 4px var(--rt-gap) 4px 0;
    text-align: left;
    font-weight: normal;
    white-space: nowrap;
  }
  .axis {
    position: relative;
    height: 14px;
  }
  .bar {
    position: absolute;
    top: 3px;
    bottom: 3px;
    border-radius: 2px;
    background: var(--rt-accent);
  }
  .bar.dns {
    background: var(--rt-var-secret);
  }
  .bar.connect {
    background: var(--rt-warn);
  }
  .bar.tls {
    background: var(--rt-danger);
  }
  .bar.ttfb {
    background: var(--rt-ok);
  }
  .value {
    width: 80px;
    padding-left: var(--rt-gap);
    text-align: right;
    font-family: var(--rt-font-mono);
    white-space: nowrap;
  }
  .total th,
  .total .value {
    border-top: 1px solid var(--rt-border);
    font-weight: 600;
  }
  .total .axis {
    border-top: 1px solid var(--rt-border);
  }
  .hidden,
  .note {
    margin: var(--rt-gap) 0 0;
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
</style>
