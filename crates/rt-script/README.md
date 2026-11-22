# rt-script

The script sandbox of RequestTailor. It runs the `before-request`, `after-response` and
`tests` scripts of a request. The API is in [`api.d.ts`](api.d.ts). The same engine runs on
every target (desktop, `rt`, the browser, iOS):
[QuickJS-ng](https://github.com/quickjs-ng/quickjs) is compiled to WebAssembly and runs in
[wasmi](https://github.com/wasmi-labs/wasmi), the interpreter the plugin host uses too.

## Layout

| Path | What |
| --- | --- |
| `engine/src/engine.c` | Guest glue. It exports `rt_init`/`rt_run`, imports `rt.host` (sends, timers) and `rt.interrupt` (limits), and runs the event loop. |
| `engine/rt-script-engine.wasm` | The built engine. It is committed and embedded with `include_bytes!`. |
| `engine/engine.json` | Pinned sources (QuickJS-ng amalgam, wasi-sdk) with SHA-256, plus the output's size and SHA-256. |
| `engine/NOTICE` | Licences of what the `.wasm` contains: QuickJS-ng, wasi-libc, dlmalloc. |
| `js/runtime/*.js` | The API: `rt`, `test`, `console`, `require`. They run once when the engine starts. |
| `js/shims/*.js` | Compatibility globals (`bru`, `req`, `res`, `pm`, `expect`). Strand B owns them. |
| `js/modules/<name>.js` | CommonJS modules for `require(name)`. Strand B owns them. |
| `src/engine.rs` | Host side: compiles the module, takes a memory snapshot and enforces the limits. |
| `tests/cases/**.json` | Data-driven cases. They run natively (`tests/cases.rs`) and in headless Chrome (`crates/rt-wasm/tests/scripts.rs`). |

All JavaScript files are embedded at build time (`build.rs`). Adding a shim or a module means
adding a file; you do not register it anywhere.

- Shims are compiled once at start-up. In every run they are evaluated after `rt` exists, as
  classic scripts in file-name order.
- Modules are wrapped as `function (exports, require, module, __filename, __dirname)`.
  `require` caches them per run.
- An unknown name throws `Error: module "<name>" is not available in RequestTailor scripts`.

## Rebuilding the engine

```sh
node scripts/build-script-engine.mjs          # rebuild, update engine.json
node scripts/build-script-engine.mjs --check  # CI: the committed bytes must match
```

The script downloads the pinned wasi-sdk and the QuickJS-ng amalgam into
`~/.cache/requesttailor/script-engine` (or `RT_CACHE_DIR`) and checks their SHA-256 before
using them. The build is reproducible: paths are mapped and there are no timestamps. Changing a
pin is a deliberate commit.

## Runs and limits

- **Fresh state per run.** Each run gets a new instance restored from the snapshot taken after
  start-up (about 3.5 MiB of memory is copied), so no state leaks between runs.
  `Math.random` is reseeded every run.
- **Time.** The default limit is 5 s per phase. It counts wall clock, including `rt.sleep` and
  `rt.sendRequest`. QuickJS polls `rt.interrupt`. When the time is up, the script unwinds with
  an uncatchable error, which keeps tests and logs written so far. A fuel budget is the
  deterministic backstop. A guest that does not return is dropped one second after the
  deadline.
- **Memory.** The default limit is 64 MiB for the whole guest memory. A failed allocation
  stops the script, and `try`/`catch` cannot swallow it.
- **Stack.** Deep recursion becomes a catchable `RangeError`, as in browsers.
- **Browser.** The guest yields to the page about every 25 ms, so a busy loop does not freeze
  it.
- **Errors.** Hitting a limit gives `ScriptOutcome.error`, for example
  `Script stopped: time limit of 5 s exceeded` or
  `Script stopped: memory limit of 64 MiB exceeded`. It never panics.
- **Imports.** WASI imports are limited to clocks, random and stdout/stderr, and stdout/stderr
  go to the log. There are no files, no environment and no network; requests go only through
  `rt.sendRequest` and the core's transport.

Cost, measured with `cargo test -p rt-script --release --test perf -- --ignored --nocapture`
on Apple M-series:

| Measurement | Result |
| --- | --- |
| Engine start, once per process, lazily on the first script (compile plus start-up) | about 38 ms |
| One trivial script | about 1.1 ms per run |

A busy loop runs at about 0.5 G fuel per millisecond.

## Case files

The format is documented in `src/cases.rs`. It mirrors `ScriptCall`/`ScriptOutcome`. In
addition to the shared format, there are:

- `skip` (a reason)
- `$comment`
- `expect.sent` (what `rt.sendRequest` sent)
- `expect.max_ms` / `expect.min_ms` (wall-clock time)

Run one case natively with `RT_CASE=<part of path> cargo test -p rt-script --test cases`.
