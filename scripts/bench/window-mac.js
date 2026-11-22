// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// macOS window watcher for the benchmark (JavaScript for Automation, run by osascript; no
// extra tools, no screen-recording permission needed: only owner, layer and bounds are read).
//
// Usage: osascript -l JavaScript window-mac.js <pid-file> <timeout-ms>
// Prints "ready <epoch-ms>" once it polls (one query takes about 5 ms on an idle machine), then waits until <pid-file> holds a pid and that
// process owns an on-screen window of normal level (layer 0) at least 100x100 points, and
// prints "window <epoch-ms>". Prints "timeout" after <timeout-ms>.

ObjC.import("CoreGraphics");
ObjC.import("Foundation");

function readPid(file) {
  const text = $.NSString.stringWithContentsOfFileEncodingError(file, $.NSUTF8StringEncoding, null);
  if (!text || text.isNil()) return 0;
  return parseInt(ObjC.unwrap(text), 10) || 0;
}

function hasWindow(pid) {
  const list = ObjC.castRefToObject(
    $.CGWindowListCopyWindowInfo($.kCGWindowListOptionOnScreenOnly | $.kCGWindowListExcludeDesktopElements, $.kCGNullWindowID),
  );
  // One bridge call instead of one per window: the bridge is the slow part.
  const mine = list.filteredArrayUsingPredicate(
    $.NSPredicate.predicateWithFormat(
      `kCGWindowOwnerPID == ${pid} AND kCGWindowLayer == 0 AND kCGWindowBounds.Width >= 100 AND kCGWindowBounds.Height >= 100`,
    ),
  );
  return mine.count > 0;
}

function run(argv) {
  const file = argv[0];
  const timeout = parseInt(argv[1] ?? "60000", 10);
  const out = (line) => {
    // stdout of osascript is only written at the end; stderr (log) is immediate.
    console.log(line);
  };
  // The first query is much slower than the rest; do it before saying "ready".
  hasWindow(0);
  readPid(file);
  out(`ready ${Date.now()}`);
  const until = Date.now() + timeout;
  while (Date.now() < until) {
    const pid = readPid(file);
    if (pid > 0 && hasWindow(pid)) {
      out(`window ${Date.now()}`);
      return "";
    }
    delay(0.005);
  }
  out("timeout");
  return "";
}
