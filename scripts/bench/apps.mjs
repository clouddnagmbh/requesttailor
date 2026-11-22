// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The desktop API clients the benchmark knows, where they are usually installed and how to
// read their version. Nothing is installed or downloaded: an app that is not found is
// reported as "not installed". Paths can be overridden with `--app <id>=<path>`.

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";

/**
 * Competitors. `engine` says how the app renders: `electron` (Chromium; its windows belong
 * to the main process, helpers are its children) or `webkit` (Tauri on macOS: WebKit XPC
 * services, attributed as in perf/README.md). `isolate(profileDir)` returns the arguments
 * that keep the app away from the user's own profile, where the app supports that.
 */
export const COMPETITORS = [
  {
    id: "postman",
    name: "Postman",
    engine: "electron",
    paths: {
      darwin: ["/Applications/Postman.app", "~/Applications/Postman.app"],
      win32: ["%LOCALAPPDATA%\\Postman\\Postman.exe"],
      linux: ["/usr/bin/postman", "/opt/Postman/Postman", "/snap/bin/postman"],
    },
  },
  {
    id: "insomnia",
    name: "Insomnia",
    engine: "electron",
    paths: {
      darwin: ["/Applications/Insomnia.app", "~/Applications/Insomnia.app"],
      win32: ["%LOCALAPPDATA%\\insomnia\\Insomnia.exe"],
      linux: ["/usr/bin/insomnia", "/opt/Insomnia/insomnia", "/snap/bin/insomnia"],
    },
  },
  {
    id: "bruno",
    name: "Bruno",
    engine: "electron",
    paths: {
      darwin: ["/Applications/Bruno.app", "~/Applications/Bruno.app"],
      win32: ["%LOCALAPPDATA%\\Programs\\bruno\\Bruno.exe"],
      linux: ["/usr/bin/bruno", "/opt/Bruno/bruno", "/snap/bin/bruno"],
    },
  },
  {
    id: "hoppscotch",
    name: "Hoppscotch",
    engine: "webkit",
    paths: {
      darwin: ["/Applications/Hoppscotch.app", "~/Applications/Hoppscotch.app"],
      win32: ["%LOCALAPPDATA%\\Hoppscotch\\Hoppscotch.exe"],
      linux: ["/usr/bin/hoppscotch", "/opt/Hoppscotch/hoppscotch"],
    },
  },
];

/** Arguments that keep an Electron app on a fresh profile and off the network (Chromium
 *  switches Electron honours): `--user-data-dir`, and a proxy on a closed loopback port, so
 *  the renderer reaches no server, no account and no telemetry. */
export function electronIsolation(profileDir) {
  return [`--user-data-dir=${profileDir}`, "--proxy-server=127.0.0.1:9"];
}

/** `~` and `%VAR%` in a configured path. */
export function expandPath(path, env = process.env, home = homedir()) {
  return path.replace(/^~(?=$|[\\/])/, home).replace(/%([A-Z_]+)%/gi, (m, name) => env[name] ?? m);
}

/** The executable inside a macOS `.app` bundle (its `CFBundleExecutable`), or `path` itself. */
export function executableOf(path, os = process.platform) {
  if (os !== "darwin" || !path.endsWith(".app")) return path;
  const plist = join(path, "Contents", "Info.plist");
  const name = plistValue(plist, "CFBundleExecutable") ?? path.split("/").pop().replace(/\.app$/, "");
  return join(path, "Contents", "MacOS", name);
}

function plistValue(plist, key) {
  if (!existsSync(plist)) return null;
  try {
    return execFileSync("plutil", ["-extract", key, "raw", "-o", "-", plist], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
  } catch {
    return null;
  }
}

/** The app's version: `CFBundleShortVersionString` of a macOS bundle, the `version` of an
 *  Electron app's `resources/app/package.json`, else `null`. */
export function versionOf(path, os = process.platform) {
  if (os === "darwin" && path.endsWith(".app")) return plistValue(join(path, "Contents", "Info.plist"), "CFBundleShortVersionString");
  for (const pkg of [join(dirname(path), "resources", "app", "package.json")]) {
    try {
      return JSON.parse(readFileSync(pkg, "utf8")).version ?? null;
    } catch {
      // Not there.
    }
  }
  return null;
}

/** Where each competitor is on this machine: `{ id, name, engine, path, executable, version }`
 *  or `{ id, name, installed: false, searched }`. `overrides` maps id to a path. */
export function discover({ os = process.platform, overrides = {}, exists = existsSync, env = process.env } = {}) {
  return COMPETITORS.map((app) => {
    const candidates = overrides[app.id] ? [overrides[app.id]] : (app.paths[os] ?? []);
    const searched = candidates.map((p) => expandPath(p, env));
    const path = searched.find((p) => exists(p));
    if (!path) return { id: app.id, name: app.name, engine: app.engine, installed: false, searched };
    return {
      id: app.id,
      name: app.name,
      engine: app.engine,
      installed: true,
      path,
      executable: executableOf(path, os),
      version: versionOf(path, os),
    };
  });
}
