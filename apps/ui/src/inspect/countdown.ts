// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand B: the ticker of a token's countdown. It ticks once per second, only while the
// view is visible and the window is in the foreground, and not at all otherwise (no timer runs
// in the background). The environment is injected so the rules are testable.

export interface TickEnv {
  /** Whether the page is visible (`document.visibilityState === "visible"`). */
  visible(): boolean;
  /** Calls `listener` when visibility changes; returns the unsubscribe function. */
  onVisibility(listener: () => void): () => void;
  setInterval(handler: () => void, ms: number): unknown;
  clearInterval(handle: unknown): void;
}

export function browserEnv(): TickEnv {
  return {
    visible: () => typeof document === "undefined" || document.visibilityState === "visible",
    onVisibility: (listener) => {
      if (typeof document === "undefined") return () => {};
      document.addEventListener("visibilitychange", listener);
      return () => document.removeEventListener("visibilitychange", listener);
    },
    setInterval: (handler, ms) => globalThis.setInterval(handler, ms),
    clearInterval: (handle) => globalThis.clearInterval(handle as ReturnType<typeof setInterval>),
  };
}

/** Calls `onTick` once per second while visible (and once at once when it becomes visible
 *  again). Returns the function that stops it for good. */
export function startCountdown(onTick: () => void, env: TickEnv = browserEnv()): () => void {
  let handle: unknown = null;
  const run = () => {
    if (handle === null && env.visible()) {
      handle = env.setInterval(onTick, 1000);
    }
  };
  const halt = () => {
    if (handle !== null) {
      env.clearInterval(handle);
      handle = null;
    }
  };
  const off = env.onVisibility(() => {
    if (env.visible()) {
      onTick();
      run();
    } else {
      halt();
    }
  });
  run();
  return () => {
    halt();
    off();
  };
}
