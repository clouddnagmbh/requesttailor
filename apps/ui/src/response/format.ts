// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Pure helpers of the response view: sizes, durations, status classes, error texts and the
// timing waterfall.

import type { TransportMode } from "../commands/backend";
import type { SendError, Timing } from "../commands/types";

/**
 * Text bodies up to this size get the pretty view (highlighted, foldable, JSON and XML
 * re-indented) and, for JSON, the tree. Both work on the whole body in the UI; larger bodies
 * stay in the backend and are shown as raw lines.
 */
export const PRETTY_LIMIT = 5 * 1024 * 1024;

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  const digits = value < 10 ? 1 : 0;
  return `${value.toFixed(digits).replace(/\.0$/, "")} ${units[unit]}`;
}

export function formatDuration(ms: number): string {
  if (ms < 1) return `${ms.toFixed(2)} ms`;
  if (ms < 100) return `${ms.toFixed(1).replace(/\.0$/, "")} ms`;
  if (ms < 1000) return `${Math.round(ms)} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

export type StatusClass = "info" | "ok" | "redirect" | "client-error" | "server-error";

export function statusClass(status: number): StatusClass {
  if (status >= 500) return "server-error";
  if (status >= 400) return "client-error";
  if (status >= 300) return "redirect";
  if (status >= 200) return "ok";
  return "info";
}

const REASONS: Record<number, string> = {
  100: "Continue",
  101: "Switching Protocols",
  200: "OK",
  201: "Created",
  202: "Accepted",
  204: "No Content",
  206: "Partial Content",
  301: "Moved Permanently",
  302: "Found",
  303: "See Other",
  304: "Not Modified",
  307: "Temporary Redirect",
  308: "Permanent Redirect",
  400: "Bad Request",
  401: "Unauthorized",
  403: "Forbidden",
  404: "Not Found",
  405: "Method Not Allowed",
  406: "Not Acceptable",
  408: "Request Timeout",
  409: "Conflict",
  410: "Gone",
  412: "Precondition Failed",
  413: "Content Too Large",
  415: "Unsupported Media Type",
  422: "Unprocessable Content",
  428: "Precondition Required",
  429: "Too Many Requests",
  500: "Internal Server Error",
  501: "Not Implemented",
  502: "Bad Gateway",
  503: "Service Unavailable",
  504: "Gateway Timeout",
};

export function reasonPhrase(status: number): string {
  return REASONS[status] ?? "";
}

/** A sentence for the user, from `SendReport.error`. */
export function describeError(error: SendError): string {
  switch (error.kind) {
    case "not-found":
      return `Request not found: ${error.message}`;
    case "prepare":
      return `The request could not be prepared: ${error.message}`;
    case "auth":
      return `Authentication failed: ${error.message}`;
    case "plugin":
      return `A plugin stopped the request: ${error.message}`;
    case "script":
      return `A pre-request script failed: ${error.message}`;
    case "blocked":
      return `Not sent: ${error.message.reason}`;
    case "transport": {
      const t = error.message;
      switch (t.kind) {
        case "invalid-url":
          return `Invalid URL: ${t.message}`;
        case "dns":
          return `The host name could not be resolved: ${t.message}`;
        case "connect":
          return `Could not connect: ${t.message}`;
        case "tls":
          return `TLS failed: ${t.message}`;
        case "timeout":
          return "The request timed out.";
        case "too-many-redirects":
          return `Too many redirects (${t.message}).`;
        case "blocked":
          return `The browser blocked the request: ${t.message}`;
        case "other":
          return t.message;
      }
    }
  }
}

export function header(headers: [string, string][], name: string): string | null {
  const lower = name.toLowerCase();
  return headers.find(([n]) => n.toLowerCase() === lower)?.[1] ?? null;
}

export type PhaseName = "dns" | "connect" | "tls" | "ttfb" | "download";

export interface Phase {
  name: PhaseName;
  label: string;
  /** Start, ms after the request was handed to the transport. */
  start: number;
  duration: number;
}

export interface Waterfall {
  phases: Phase[];
  total: number;
  /** Phases the transport could not see, with the reason for the user. */
  hidden: { name: PhaseName; label: string; reason: string }[];
}

const LABELS: Record<PhaseName, string> = {
  dns: "DNS",
  connect: "Connect",
  tls: "TLS",
  ttfb: "Waiting (TTFB)",
  download: "Download",
};

/**
 * DNS, connect and TLS are consecutive phases; `ttfb` runs from the start to the first
 * response byte (so it contains them); the download runs from there to `total`.
 */
export function waterfall(timing: Timing, total: number, mode: TransportMode, url: string): Waterfall {
  const phases: Phase[] = [];
  const hidden: Waterfall["hidden"] = [];
  let at = 0;
  for (const name of ["dns", "connect", "tls"] as const) {
    const d = timing[name];
    if (d === null) {
      const reason =
        mode === "browser"
          ? "not visible in the browser"
          : name === "tls" && url.startsWith("http:")
            ? "not used (plain HTTP)"
            : "not measured";
      hidden.push({ name, label: LABELS[name], reason });
      continue;
    }
    phases.push({ name, label: LABELS[name], start: at, duration: d });
    at += d;
  }
  const ttfb = Math.max(timing.ttfb, at);
  phases.push({ name: "ttfb", label: LABELS.ttfb, start: at, duration: ttfb - at });
  const end = Math.max(total, ttfb);
  phases.push({ name: "download", label: LABELS.download, start: ttfb, duration: end - ttfb });
  return { phases, total: end, hidden };
}
