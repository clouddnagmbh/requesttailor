# Performance budgets

The budgets are release gates (product principle 4): a build fails when one is exceeded or not
measured. Thresholds live only in [`budgets.toml`](budgets.toml); this file says what is measured
and how.

| Metric | Budget | Measured by |
| --- | --- | --- |
| `cold_start` | ≤ 500 ms | desktop release build, median of 9 starts |
| `idle_memory` | ≤ 150 MB | desktop release build, app + webview processes, median of 3 starts |
| `large_response_frame` | ≤ 20 ms | p95 frame interval while scrolling a 100 MB body, Chrome |
| `send_keystrokes` | ≤ 2 | keystrokes from app start to a sent request, Chrome |
| `semantic_diff` | ≤ 200 ms | semantic diff of 1000 requests (100 changed), release build, median of 9 |

```sh
npm run build -w apps/desktop          # the release binary the desktop part starts
node scripts/perf/run.mjs --check      # measures everything, writes perf/results.json, checks
node scripts/perf/run.mjs --only desktop --runs 10 --memory-runs 3
node scripts/check-perf-budgets.mjs --require-all   # the gate on an existing perf/results.json
```

`run.mjs` writes `perf/results.json` (metric → number, what the gate reads) and
`perf/report.json` (every sample, the per-process memory breakdown, machine and load). Both are
gitignored. Nothing is sent anywhere: measurements stay on the machine or in the CI job's
artifacts (no telemetry).

Measure on a quiet machine with the screen unlocked. `run.mjs` warns when the load average
exceeds the number of CPUs and refuses the desktop part when macOS reports a locked screen (a
locked screen renders no frames, so the first frame never comes).

## Cold start (`cold_start`)

**From process spawn to the first interactive frame**: the shell has been painted once and its
main thread is free to handle input.

- The release binary (`npm run build -w apps/desktop`, frontend embedded) is started 10 times;
  the first start is a warm-up and discarded (a new binary's first launch pays for code-signing
  checks and cold disk caches). The result is the median of the other 9. So this is a start
  with warm OS caches, as when a user opens the app the second time; a start right after boot
  is slower and not measured.
- Measurement mode: `RT_PERF_DIR` (a fresh temporary folder per start) and `RT_PERF_T0` (Unix
  time in ms, taken right before the spawn). The app writes `marks.log` there; outside this mode
  it writes nothing (`apps/desktop/src-tauri/src/recent.rs`, `perf` module).
- End point: after mounting, the UI waits for an animation frame and then one more task
  (`afterFirstFrame` in `apps/ui/src/recent/startup.ts`), then asks the backend for the recent
  collections. The Rust side records that call as `first-frame`. At that moment a frame has
  been rendered and the JavaScript main thread is idle, i.e. a key press would be handled. The
  time includes process start, Tauri and webview start-up, loading and running the frontend,
  mounting the shell, its first frame and one IPC round trip.
- `afterFirstFrame` has no timeout on purpose: a hidden window renders no frames, and a timer
  would end the interval without a frame.
- Also recorded (report only): `core-ready` (the Rust core is set up, before the window exists)
  and `collection-open` (the sample collection is reopened from the recent list).

## Idle memory (`idle_memory`)

**Memory of the app and its webview processes, idle, with one collection open.**

- The app reopens a fixed sample collection, [`testdata/conformance/collection`](../testdata/conformance/collection),
  from a recent list that the measurement writes into `RT_PERF_DIR` (the user's own list is not
  touched). Measured in the last 3 of the 10 starts: at least 3 s after `collection-open`, then
  sampled every second until three samples differ by less than 2 % (at most 20 s). The result is
  the median of the 3 starts.
- Counted processes and the number the gate uses, per OS (`scripts/perf/memory.mjs`):

  | OS | processes | gate (`ownMB`) | also reported (`rssMB`) |
  | --- | --- | --- | --- |
  | macOS | `rt-desktop` and the WebKit XPC services started for it: `com.apple.WebKit.WebContent`, `.Networking`, `.GPU` | `phys_footprint` from `footprint` (what Activity Monitor shows as "Memory") | RSS from `ps` |
  | Linux | `rt-desktop` and every descendant (`WebKitWebProcess`, `WebKitNetworkProcess`, sandbox helpers) | PSS from `/proc/<pid>/smaps_rollup` | `VmRSS` |
  | Windows | `rt-desktop.exe` and every descendant (`msedgewebview2.exe`) | private working set (`Win32_PerfRawData_PerfProc_Process.WorkingSetPrivate`, Task Manager's "Memory") | working set |

- Why not the RSS sum: RSS counts shared pages (the WebKit and system frameworks, the dyld shared
  cache) once per process, so summing it over four processes counts the same pages up to four
  times, and they are shared with every other app using the system webview. `phys_footprint`,
  PSS and the private working set charge each process for its own memory (PSS splits shared
  pages between their users). The RSS sum is in the report for comparison.
- macOS attribution: the WebKit processes are XPC services whose parent is `launchd`, so they
  are not in the app's process tree. They are attributed as the WebKit processes that did not
  exist before the app started; the report warns unless there is exactly one WebContent process.
  Another app starting a web view during the measurement would be counted too (an overcount,
  never an undercount).

## 100 MB response (`large_response_frame`)

**p95 of the frame intervals while scrolling a 100 MB response body.**

- Playwright in headless Chrome (1280×800), one worker, `apps/web/e2e/perf/large-response.perf.ts`
  with `e2e/perf/playwright.perf.config.ts`. Two variants, the budget takes the worse p95:
  - the mock backend's virtual 100 MB body (`demo/large.yml`): the UI alone,
  - the real core (rt-wasm) holding a 100 MB body fetched from `rt-testserver` (`/bytes/104857600`).
- The scroll is driven from inside the page, one step per animation frame, in three phases of
  240 frames each: a fast wheel scroll (600 px per frame), a jump to a random line of the whole
  body every frame (uncached lines each time, like dragging the scrollbar thumb), and PageDown
  every frame. The frame time is the interval between consecutive `requestAnimationFrame`
  callbacks.
- A negative control in the same test burns 25 ms per frame and must show up as long intervals
  (measured: p95 33.4 ms), so the method does see stutter.

### The threshold: 20 ms instead of the proposed 16.7 ms

The proposal was 16.7 ms, one frame at 60 Hz. The intent is confirmed ("no dropped frame at
60 Hz"), the number is adjusted: rAF intervals of a perfectly smooth run sit at the refresh
period plus timer jitter (measured: p50 16.7 ms, p95 16.7 to 16.8 ms, max 16.8 ms with nothing
dropped), so 16.7 ms would fail smooth runs at random. A dropped frame shows as an interval of
at least two periods (33.3 ms). Intervals are bimodal, so any threshold between the two means
the same thing; 20 ms leaves room for jitter on slower CI machines: **p95 ≤ 20 ms means at
least 95 % of all frames were on time at 60 Hz.** Headless Chrome renders at 60 Hz; on a 120 Hz
display the same budget would allow one dropped 8.3 ms frame, which is why it is measured
headless.

## Keystrokes (`send_keystrokes`)

**Keystrokes from app start to a sent request**, budget "under three", i.e. at most 2.

`apps/web/e2e/recent/recent.spec.ts` (mock backend) and `recent.e2e.ts` (real web backend:
rt-wasm, OPFS, IndexedDB) open a collection and a request, reload the page (= restart), wait
until the last collection and its last active request are back, and press Mod+Enter once; the
response must arrive. The result is the larger count of the two (measured: 1). The desktop
reopens the same way (`recent_collections`/`open_recent`, covered by
`apps/desktop/src-tauri/tests/recent.rs`; the start-up logic in `apps/ui/src/recent` is shared).

## Semantic diff (`semantic_diff`, wave 12)

**Median time of `rt_format::diff::diff` over 9 runs** on a generated collection: 1000 requests
in 20 folders, every tenth with a new URL (100 changed files). `crates/rt-format/tests/diff_perf.rs`
in a release build (`run.mjs --only diff`) writes `diff.json`; it is native code, so every OS
measures it and the worst value counts. A second test in the same file checks what the diff
finds; it is `#[ignore]`d until wave 12 strand A replaces the file-level interface `diff` and
then runs with `cargo test`. The measurement runs from the interface commit on, so the budget
is gated before strand A lands and stays gated after. The WASM build of the same diff is not measured yet (later wave).

## CI

`.github/workflows/ci.yml`:

- `perf-measure` (macOS, Windows, Linux under `xvfb-run`): builds the desktop release binary and
  runs `run.mjs --only <parts> --check`; an exceeded budget fails the job. Linux runs all parts,
  macOS and Windows the desktop and diff parts: frame times and keystrokes are browser measurements of the
  shared UI in Chrome, the same code on every OS, so they run once. Results are uploaded as
  artifacts `perf-<os>`.
- `perf`: requires the results of all three OSes (a missing one fails), merges them with the
  worst value per metric (`scripts/perf/merge.mjs`) and runs the gate with `--require-all`. A
  metric no OS measured fails the build; nothing is skipped silently. `ci-ok` depends on both.
