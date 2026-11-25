// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The script debugger's side inside the engine (wave 19, strand Y; `rt_script::debug`).
//
// Inert in every normal run: it only registers a factory with the runtime glue, which calls it
// from `begin` when the run message carries `debug`. A debug run instruments the target script
// (crates/rt-script/src/debug/instrument.rs): `__rt_b(line, locals, closure, ev)` before each
// statement, `__rt_e(name)` / `__rt_x()` around the bodies of ordinary functions. `ev` is a
// direct `eval` inside the paused scope, so the values (and an evaluated expression) are read
// where the script is stopped. A pause is one host call (`{op: "debug-pause", ...}`); the
// engine stays suspended in it until the host answers with the next command.

(function () {
  "use strict";

  const json = JSON;
  const PREVIEW = 200;

  globalThis.__rt_internal.debugHook(function (opts, env) {
    const bps = new Map();
    const setBreakpoints = (list) => {
      bps.clear();
      for (const b of list || []) bps.set(b.line, b.condition === undefined ? null : b.condition);
    };
    setBreakpoints(opts.breakpoints);

    const origin = String(opts.origin || "request");
    const globals = String(opts.globals || "").split(",").filter(Boolean);
    const names = [];
    const lines = [0];
    let mode = opts.stop_on_entry ? "entry" : "run";
    let stepDepth = 0;
    let skip = null;
    const now = () => performance.now();

    function safeJson(value) {
      const seen = [];
      try {
        const text = json.stringify(value, function (key, v) {
          if (typeof v === "bigint") return `${v}n`;
          if (typeof v === "function") return `[Function ${v.name || ""}]`;
          if (typeof v === "object" && v !== null) {
            if (seen.includes(v)) return "[Circular]";
            seen.push(v);
            if (v instanceof Map) return { "Map": [...v] };
            if (v instanceof Set) return { "Set": [...v] };
          }
          return v;
        });
        return text === undefined ? String(value) : text;
      } catch (e) {
        return String(value);
      }
    }

    function describe(name, v) {
      const kind = v === null ? "null" : Array.isArray(v) ? "array" : typeof v;
      let preview;
      let children = 0;
      switch (kind) {
        case "string":
          preview = json.stringify(v);
          break;
        case "function":
          preview = `function ${v.name || "(anonymous)"}()`;
          break;
        case "object":
        case "array":
          preview = safeJson(v);
          try {
            children = Array.isArray(v) ? v.length : Object.keys(v).length;
          } catch (e) {
            children = 0;
          }
          break;
        default:
          preview = String(v);
      }
      if (preview.length > PREVIEW) preview = `${preview.slice(0, PREVIEW - 1)}…`;
      return { name: String(name), kind, preview, children };
    }

    function valuesOf(csv, ev) {
      const out = [];
      const seen = new Set();
      for (const name of csv ? csv.split(",") : []) {
        if (!name || seen.has(name)) continue;
        seen.add(name);
        try {
          out.push(describe(name, ev(name)));
        } catch (e) {
          // "x is not defined": not a variable of this scope (the instrumenter guesses).
          if (e && /not initialized|before initialization/.test(String(e.message))) {
            out.push({ name, kind: "uninitialized", preview: "<not initialized>", children: 0 });
          }
        }
      }
      return out;
    }

    function rtScope() {
      const rt = globalThis.rt;
      const out = [];
      if (!rt) return out;
      const put = (name, f) => {
        try {
          out.push(describe(name, f()));
        } catch (e) {
          // not available in this phase
        }
      };
      put("request", () => ({
        method: rt.request.method,
        url: rt.request.url,
        headers: rt.request.headers.entries(),
        body: rt.request.body,
      }));
      if (rt.response) {
        put("response", () => ({
          status: rt.response.status,
          headers: rt.response.headers.entries(),
          time: rt.response.time,
          size: rt.response.size,
          body: rt.response.text(),
        }));
      }
      put("vars", () => rt.vars.toObject());
      put("info", () => rt.info);
      return out;
    }

    function evaluate(expression, ev) {
      try {
        return describe(expression, ev(String(expression)));
      } catch (e) {
        const text = e && e.message !== undefined ? `${e.name || "Error"}: ${e.message}` : String(e);
        return { name: String(expression), kind: "error", preview: text.slice(0, PREVIEW), children: 0 };
      }
    }

    function pause(reason, line, id, locals, closure, ev) {
      const depth = names.length;
      // Time spent here is the debugger's, not the script's: the host adds it back to the
      // time limit (`busy`, in ms).
      let mark = now();
      const frames = [];
      for (let d = depth; d >= 0; d--) {
        frames.push({ function: d === 0 ? origin : names[d - 1], line: lines[d] || 0 });
      }
      const scopes = [{ name: "local", values: valuesOf(locals, ev) }];
      const outer = valuesOf(closure, ev);
      if (outer.length) scopes.push({ name: "closure", values: outer });
      scopes.push({ name: "global", values: valuesOf(globals.join(","), ev) });
      scopes.push({ name: "rt", values: rtScope() });
      const view = {
        line,
        reason,
        frames,
        scopes,
        output: env.logs().map((l) => l.text),
      };
      let message = { op: "debug-pause", pause: view, busy: now() - mark };
      for (;;) {
        const reply = env.call(message);
        mark = now();
        if (reply.breakpoints) setBreakpoints(reply.breakpoints);
        switch (reply.cmd) {
          case "continue":
            mode = "run";
            break;
          case "step-over":
            mode = "over";
            stepDepth = depth;
            break;
          case "step-into":
            mode = "into";
            break;
          case "step-out":
            mode = "out";
            stepDepth = depth;
            break;
          case "evaluate":
            message = { op: "debug-pause", pause: view, evaluated: evaluate(reply.expression, ev) };
            message.busy = now() - mark;
            continue;
          default:
            message = { op: "debug-pause", pause: view, busy: 0 };
            continue;
        }
        break;
      }
      skip = { line, id, depth };
    }

    function brk(line, id, locals, closure, ev) {
      const depth = names.length;
      lines[depth] = line;
      if (skip !== null) {
        // Another statement on the line we stopped at is not a new stop; the same statement
        // again (a loop) is.
        const same = skip.line === line && skip.depth === depth && skip.id !== id;
        skip = null;
        if (same) return;
      }
      let reason = null;
      if (mode === "entry") {
        reason = "entry";
      } else {
        if (bps.has(line)) {
          const condition = bps.get(line);
          if (condition === null) {
            reason = "breakpoint";
          } else {
            try {
              if (ev(condition)) reason = "breakpoint";
            } catch (e) {
              reason = "breakpoint";
            }
          }
        }
        if (reason === null) {
          if (mode === "into" || (mode === "over" && depth <= stepDepth) || (mode === "out" && depth < stepDepth)) {
            reason = "step";
          }
        }
      }
      if (reason !== null) pause(reason, line, id, locals, closure, ev);
    }

    env.define("__rt_b", brk);
    env.define("__rt_e", (name) => {
      names.push(String(name));
    });
    env.define("__rt_x", () => {
      names.pop();
      lines.length = names.length + 1;
    });
  });
})();
