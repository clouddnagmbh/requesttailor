<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# Benchmark

RequestTailor's performance budgets ([`perf/budgets.toml`](../perf/budgets.toml)) measured
next to the other desktop API clients installed on the same machine, by one script, the same
way for every app. Results are committed in [`results/`](results/), one JSON file (every
sample, the machine, the versions) and one Markdown table per run; the website's
[benchmark page](../website/benchmark/index.html) shows the latest one.

The benchmark installs nothing, downloads nothing and sends nothing anywhere. Apps that are
not installed are reported as "not installed".

## Run it

```sh
npm ci --ignore-scripts
npm run build -w apps/desktop          # the RequestTailor release build that is measured
node scripts/bench/run.mjs --browser   # all apps; writes results/<date>-<os>.json and .md
```

| Option | Meaning |
| --- | --- |
| `--apps <list>` | `requesttailor,postman,insomnia,bruno,hoppscotch` (default: all) |
| `--runs <n>` | starts per app, the first one is discarded (default 10) |
| `--memory-runs <n>` | of those, the last `n` also measure memory (default 3) |
| `--rt-binary <path>` | the RequestTailor binary (default `<target>/release/rt-desktop`, `CARGO_TARGET_DIR` is honoured) |
| `--app <id>=<path>` | where an app is: the `.app` bundle on macOS, the executable elsewhere |
| `--browser` | also measure RequestTailor's browser metrics (100 MB response, keystrokes) with `scripts/perf/run.mjs --only frames,keystrokes` (needs Chrome and the wasm toolchain) |
| `--perf-results <file>` | take those from an existing `scripts/perf/run.mjs` result instead |
| `--out-dir <dir>` | default `bench/results` |
| `--no-website` | do not update `website/benchmark/index.html` |

`node scripts/bench/report.mjs [<result.json>] [--website website/benchmark/index.html]
[--markdown <file>]` regenerates the tables from a result (default: the newest in
`results/`). The script's tests (`node --test scripts/bench/*.test.mjs`, part of
`npm run test:scripts`) check that the website shows the newest committed result with the
budgets of `perf/budgets.toml`.

Default app locations:

| App | macOS | Windows | Linux |
| --- | --- | --- | --- |
| Postman | `/Applications/Postman.app`, `~/Applications/Postman.app` | `%LOCALAPPDATA%\Postman\Postman.exe` | `/usr/bin/postman`, `/opt/Postman/Postman`, `/snap/bin/postman` |
| Insomnia | `/Applications/Insomnia.app`, `~/Applications/…` | `%LOCALAPPDATA%\insomnia\Insomnia.exe` | `/usr/bin/insomnia`, `/opt/Insomnia/insomnia`, `/snap/bin/insomnia` |
| Bruno | `/Applications/Bruno.app`, `~/Applications/…` | `%LOCALAPPDATA%\Programs\bruno\Bruno.exe` | `/usr/bin/bruno`, `/opt/Bruno/bruno`, `/snap/bin/bruno` |
| Hoppscotch | `/Applications/Hoppscotch.app`, `~/Applications/…` | `%LOCALAPPDATA%\Hoppscotch\Hoppscotch.exe` | `/usr/bin/hoppscotch`, `/opt/Hoppscotch/hoppscotch` |

## Before you run it

- **A quiet machine.** Close other apps, wait until builds are done, keep the screen
  unlocked and the machine on power. The script records the load average at the start and
  warns when it exceeds the number of CPUs; such results are pessimistic for every app.
- **Quit the apps to be measured.** An app that is already running is not measured (the
  script never stops processes it did not start).
- Windows will open and close on screen for a few minutes; do not use the machine meanwhile.

## What is measured

| Column | Definition | Apps |
| --- | --- | --- |
| First window | Process spawn until the app's process owns an on-screen window of normal level, at least 100×100 points. macOS: CoreGraphics' window list, polled about every 5 ms by `scripts/bench/window-mac.js` (JavaScript for Automation, no extra tools, no screen-recording permission). Windows: `MainWindowHandle`, polled. Linux: `xdotool` if installed, otherwise not measured. An app that shows an empty window before its content is favoured, never penalised. | all |
| First interactive frame (`cold_start`) | The budget's own definition ([perf/README.md](../perf/README.md#cold-start-cold_start)): the release build in measurement mode records when its first frame is rendered and its main thread is free for input. Taken from the same starts as "first window". | RequestTailor |
| Idle memory (`idle_memory`) | As in [perf/README.md](../perf/README.md#idle-memory-idle_memory), with `scripts/perf/memory.mjs`: the app and its helper and web view processes. Tauri apps (RequestTailor, Hoppscotch): the app's process tree plus the WebKit XPC services that appeared after the start. Electron apps: their helpers; on macOS exactly the processes of the app bundle started for this run (the crashpad handler that macOS re-parents included), so web views of other apps are never counted. macOS `phys_footprint`, Linux PSS, Windows private working set. Sampled at least 3 s after the first window (RequestTailor: after its collection opened), then every second until three samples differ by less than 2 % (at most 20 s). RSS sums are in the JSON for comparison. | all |
| 100 MB response, p95 frame (`large_response_frame`) | `scripts/perf/run.mjs --only frames`: headless Chrome, RequestTailor's UI. | RequestTailor |
| Keystrokes to a sent request (`send_keystrokes`) | `scripts/perf/run.mjs --only keystrokes`. | RequestTailor |

The last two need RequestTailor's browser build and its test hooks; other apps would need
their UI scripted, which this benchmark does not do. They are "n/a" there.

## Method

- **Same machine, one script.** Every app is measured in the same run, one after the
  other. The JSON records the hardware model, CPU, memory, OS version and build, Node
  version, load average, each app's version (macOS: `CFBundleShortVersionString`;
  RequestTailor: the workspace version and Git commit) and every sample.
- **Starts and statistics.** `--runs` starts per app (default 10). The first one is a warm-up
  and discarded: a binary started for the first time pays for code-signing checks and cold
  disk caches. The result is the **median** of the remaining starts; memory is the median of
  the last `--memory-runs` (default 3).
- **Caches.** Starts happen with warm operating-system caches, like opening the app for the
  second time that day. Starts right after boot are slower for every app and not measured.
  The scripts do not purge caches (that needs administrator rights).
- **Profiles and network.** RequestTailor runs with a temporary data folder (`RT_DATA_DIR`)
  and reopens the fixed sample collection `testdata/conformance/collection`, as in the budget
  measurement. Electron apps (Postman, Insomnia, Bruno) get a fresh, empty profile
  (`--user-data-dir`) shared by the starts of the series, and their web content goes through
  a proxy on a closed loopback port (`--proxy-server=127.0.0.1:9`), so the renderer reaches no
  account and no server. Updaters outside the renderer may still try to connect. No account
  is used; an app that asks for a sign-in is measured to that screen. Hoppscotch (Tauri) has
  no profile option and runs with its own.
- **Stopping.** After each start the script stops the processes it started (SIGTERM, then
  SIGKILL after 5 s), including helpers that the OS re-parented, and waits a second before
  the next start.

## Limits

- "First window" is not "ready to use": an app may show its window and render its content
  later. That favours the other apps; RequestTailor's "first interactive frame" is the
  stricter number and is the one the budget gates.
- One machine per result file. Results from different machines are not comparable; compare
  rows within one file.
- Only the apps installed on the machine are measured; nothing is downloaded to fill gaps.
