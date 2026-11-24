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
| `compare_view` | ≤ 1000 ms | comparison view of 1000 requests (100 changed) with both runs' answers, mock backend, Chrome, median of 5 |
| `git_graph` | ≤ 1500 ms | commit graph of 10 000 commits: first rows visible after opening, mock backend, Chrome, median of 5 |
| `git_graph_frame` | ≤ 20 ms | p95 frame interval while scrolling that commit graph, Chrome |
| `git_graph_large` | ≤ 1500 ms | commit graph of 100 000 commits: first page natively plus first rows visible, median of 5 (pending: wave 15, strand C) |
| `snapshot_compare` | ≤ 150 ms | 1000 stored responses compared with their snapshots, 100 differing, JSON diff included, native, release, median of 9 (pending: wave 17, strand B) |
| `share_url_size` | ≤ 2000 chars | length of the share link of every request of the Bruno corpus, 95th percentile (pending: wave 17, strand C) |
| `observer_overhead` | ≤ 1 ms | time three do-nothing observers add to one send, median over 500 sends |
| `watch_resend` | ≤ 500 ms | `rt send --watch`: file change to request on the wire, median of 9 (pending: wave 17, strand D) |
| `git_status_large` | ≤ 300 ms | working copy of 50 000 tracked files, 100 changed, natively, release, median of 9 (pending: wave 15, strand B) |

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

**Memory of the app and its webview processes, idle, with a folder of ten collections in the
sidebar.**

- Since wave 14 the app reopens a temporary folder with ten copies (`c01` … `c10`) of a fixed
  sample collection, [`testdata/conformance/collection`](../testdata/conformance/collection),
  from a recent list that the measurement writes into `RT_PERF_DIR` (the user's own list is not
  touched). The app finds all ten for the sidebar and reads the first; `--collections 1` measures
  the collection alone (`scripts/perf/desktop.mjs`, `collectionRoot`). The cold start uses the
  same folder. Measured in the last 3 of the 10 starts: at least 3 s after `collection-open`, then
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

## Comparison view (`compare_view`, wave 14)

`node scripts/perf/run.mjs --only compare` runs `apps/web/e2e/perf/compare.measure.ts`
(config `compare.perf.config.ts`, Vite on `RT_PERF_COMPARE_PORT`, default 48274) against the
mock backend: a collection of 1000 requests in 20 folders, a commit, 100 requests changed, a
second commit; then the comparison view compares `HEAD~1` with `HEAD`. One sample is the time
from the click on "Compare" until the numbers show 100 changes, both runs' answers are in, the
list shows rows and the first change is shown side by side. One warm-up (reported as `first`),
then the median of five. The list is virtualized: the measurement also checks that fewer than 80
rows are in the DOM while all 1005 are listed. The mock measures the view, not a network or the
Rust core (the semantic diff in Rust has its own budget, `semantic_diff`).

## Commit graph (`git_graph`, `git_graph_frame`, wave 14)

`node scripts/perf/run.mjs --only graph` runs `apps/web/e2e/perf/graph.measure.ts` (config
`graph.perf.config.ts`, Vite on `RT_PERF_GRAPH_PORT`, default 48256) against the mock backend
with a synthetic repository of 10 000 commits (`?graph=10000`, `apps/ui/src/git/graph/synthetic.ts`:
feature branches, merges, an octopus merge now and then, a second root, tags).

- `git_graph`: one sample is the time from the click on "Git: History" in the palette until at
  least ten rows are on screen (the frame after they were inserted). The view loads the first
  page (2000 commits), lays out its lanes and renders the rows in view. One warm-up, then the
  median of five openings.
- `git_graph_frame`: the frame-interval method of `large_response_frame`, in three phases of 240
  frames each: 900 px of scrolling per frame through the whole history (the remaining pages load
  on the way), a jump to a random place every frame, and PageDown on the list every frame. The
  p95 over all intervals is the result. The measurement also checks that the list stays
  virtualised (fewer than 120 rows in the DOM) and that all 10 000 commits were loaded.

The mock measures the view, not Git: reading the graph natively is timed by
`crates/rt-git/tests/graph.rs` (`ten_thousand_commits_page_quickly`).

## Git client (`git_status_large`, `git_graph_large`, wave 15)

Both budgets carry `pending = "wave 15, strand …"` in `budgets.toml`: until that strand builds
the measurement, a missing value does not fail `--require-all`, a measured one must hold. The
strand removes `pending` in the same commit.

- `git_status_large` (strand B, `node scripts/perf/run.mjs --only status`, `./status.mjs`): a
  synthetic repository in a temporary folder with 50 000 tracked files in nested folders, 100 of
  them changed (modified, new, deleted). One sample is one `workingCopy` of the active
  repository in a release build of `rt-git` (`crates/rt-git/examples/perf_status.rs`), warm
  (the index and the file system cache read once before); the median of nine.
- `git_graph_large` (strand C, `node scripts/perf/run.mjs --only graph-large`,
  `./graph-large.mjs`): a synthetic repository of 100 000 commits (branches and merges as for
  `git_graph`). One sample is the first graph page natively in a release build
  (`crates/rt-git/examples/perf_graph.rs`) plus the time until the first rows of the commit
  graph are on screen; the median of five.

## Developer Delight (`snapshot_compare`, `share_url_size`, `observer_overhead`, `watch_resend`, wave 17)

All four carry `pending = "wave 17, strand …"` in `budgets.toml` (rule of wave 15): a missing
value does not fail `--require-all` until the strand builds the measurement and removes
`pending` in the same commit; a measured value must hold. `share_url_size` is an estimate the
strand confirms after its first measurement; `observer_overhead` was measured by strand A
(0.003 ms, threshold of 1 ms confirmed, `pending` removed).

- `snapshot_compare` (strand B, `node scripts/perf/run.mjs --only snapshots`, `./snapshots.mjs`):
  1000 stored responses of 2 KB JSON, each with a snapshot with 20 ignored paths, 100 of them
  differing; one sample compares all of them, JSON diff included, in a release build of
  `rt-app`; the median of nine.
- `share_url_size` (strand C, `--only share`, `./share.mjs`): the length of the link for every
  request of the Bruno collections in `testdata/collections/bruno`, 95th percentile; links
  longer than 8000 characters count as refused.
- `observer_overhead` (strand A, `--only observe`, `./observe.mjs`): three observers that do
  nothing against `rt-testserver`; the median difference to a send without observers over 500
  sends (pairs of the same request, alternating; `crates/rt-core/tests/observer_perf.rs`, release
  build).
- `watch_resend` (strand D, `--only watch`, `./watch.mjs`, `crates/rt-cli/tests/watch.rs`): from
  the change of a request file to the request on the wire, `rt-testserver`, median of nine.
