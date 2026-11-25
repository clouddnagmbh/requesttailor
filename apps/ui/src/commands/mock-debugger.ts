// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `DebuggerApi` of the mock backend (wave 19, strand Y): a small simulated session for UI tests
// (`?backend=mock`). Every script is the same four lines; the mock stops at breakpoints and
// steps line by line, keeps variables as the lines would set them, evaluates `name` and simple
// sums, and masks the secret `token` like the core does.

import { BackendError, type DebuggerApi } from "./backend";
import type { Breakpoint, DebugCommand, DebugPause, DebugState, DebugTarget, DebugValue } from "./types";

/** The script the mock debugs (also shown by the view through `MOCK_DEBUG_SOURCE`). */
export const MOCK_DEBUG_SOURCE = [
  "const who = 'debugger';",
  "const n = 6;",
  "rt.request.headers.set('X-Debugged', who + n);",
  "console.log('token', rt.vars.get('token'));",
].join("\n");

const LINES = 4;

interface Session {
  target: DebugTarget;
  line: number;
  breakpoints: Breakpoint[];
  state: DebugState;
}

function value(name: string, v: string | number): DebugValue {
  return typeof v === "number"
    ? { name, kind: "number", preview: String(v) }
    : { name, kind: "string", preview: JSON.stringify(v) };
}

function scopeAt(line: number): DebugValue[] {
  const values: DebugValue[] = [];
  if (line > 1) values.push(value("who", "debugger"));
  if (line > 2) values.push(value("n", 6));
  return values;
}

function pauseAt(line: number, reason: string, evaluated?: DebugValue): DebugPause {
  return {
    line,
    reason,
    frames: [{ function: "<script>", line }],
    scopes: [{ name: "Local", values: scopeAt(line) }],
    ...(evaluated ? { evaluated } : {}),
  };
}

export function mockDebugger(): DebuggerApi {
  const sessions = new Map<number, Session>();
  let next = 0;

  const get = (id: number): Session => {
    const s = sessions.get(id);
    if (!s) throw new BackendError("shell", `there is no debug session ${id}`);
    return s;
  };

  const stopAt = (s: Session, line: number, reason: string): DebugState => {
    s.line = line;
    s.state = { session: s.state.session, status: "paused", pause: pauseAt(line, reason), output: s.state.output };
    return s.state;
  };

  const finish = (s: Session): DebugState => {
    const output = [...(s.state.output ?? []), "token ***"];
    s.state = { session: s.state.session, status: "finished", output };
    return s.state;
  };

  const runTo = (s: Session, from: number): DebugState => {
    for (let line = from; line <= LINES; line++) {
      if (s.breakpoints.some((b) => b.line === line)) return stopAt(s, line, "breakpoint");
    }
    return finish(s);
  };

  const evaluate = (s: Session, expression: string): DebugValue => {
    const known = new Map(scopeAt(s.line).map((v) => [v.name, v]));
    const text = expression.trim();
    if (text.includes("token")) return { name: text, kind: "string", preview: '"***"' };
    const direct = known.get(text);
    if (direct) return { ...direct, name: text };
    const sum = /^(\w+)\s*([*+-])\s*(\d+)$/.exec(text);
    const left = sum ? known.get(sum[1]!) : undefined;
    if (sum && left?.kind === "number") {
      const a = Number(left.preview);
      const b = Number(sum[3]);
      const r = sum[2] === "*" ? a * b : sum[2] === "+" ? a + b : a - b;
      return { name: text, kind: "number", preview: String(r) };
    }
    return { name: text, kind: "error", preview: `ReferenceError: ${text} is not defined` };
  };

  return {
    async start(target) {
      const id = ++next;
      const s: Session = {
        target,
        line: 0,
        breakpoints: [...target.breakpoints],
        state: { session: id, status: "running", output: [] },
      };
      sessions.set(id, s);
      return target.stop_on_entry ? stopAt(s, 1, "entry") : runTo(s, 1);
    },
    async command(session, command: DebugCommand) {
      const s = get(session);
      if (s.state.status !== "paused") return s.state;
      switch (command.kind) {
        case "continue":
          return runTo(s, s.line + 1);
        case "step-over":
        case "step-into":
          return s.line >= LINES ? finish(s) : stopAt(s, s.line + 1, "step");
        case "step-out":
          return runTo(s, s.line + 1);
        case "evaluate": {
          const evaluated = evaluate(s, command.expression);
          s.state = { ...s.state, pause: pauseAt(s.line, s.state.pause?.reason ?? "step", evaluated) };
          return s.state;
        }
        case "set-breakpoints":
          s.breakpoints = [...command.breakpoints];
          return s.state;
        case "abort":
          s.state = { session, status: "aborted", output: s.state.output };
          return s.state;
      }
    },
    async state(session) {
      return get(session).state;
    },
    async stop(session) {
      sessions.delete(session);
    },
  };
}
