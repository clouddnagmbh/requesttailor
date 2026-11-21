// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Per-device conveniences of the shell (theme, panel sizes, last open tabs) in localStorage.
// Storage may be missing or throw (private windows, blocked site data); then nothing is
// remembered and the shell still works.

const PREFIX = "rt.shell.";

export function load<T>(key: string, fallback: T, valid: (v: unknown) => v is T): T {
  try {
    const raw = globalThis.localStorage?.getItem(PREFIX + key);
    if (raw == null) return fallback;
    const value: unknown = JSON.parse(raw);
    return valid(value) ? value : fallback;
  } catch {
    return fallback;
  }
}

export function store(key: string, value: unknown): void {
  try {
    if (value === undefined) globalThis.localStorage?.removeItem(PREFIX + key);
    else globalThis.localStorage?.setItem(PREFIX + key, JSON.stringify(value));
  } catch {
    // Not remembered; nothing else depends on it.
  }
}

export const isNumber = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);
