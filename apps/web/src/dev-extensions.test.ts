// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The demo extension is for the dev server only: only `main.ts` may import it, and only behind
// `import.meta.env.DEV` (so a build drops it).

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? sources(path) : /\.(ts|svelte)$/.test(name) ? [path] : [];
  });
}

describe("demo extension", () => {
  it("is imported only by main.ts, behind import.meta.env.DEV", () => {
    const importers = sources("src").filter((f) => /dev-extensions/.test(readFileSync(f, "utf8")) && !f.includes("dev-extensions"));
    expect(importers).toEqual([join("src", "main.ts")]);
    const main = readFileSync(join("src", "main.ts"), "utf8");
    expect(main).toMatch(/if \(import\.meta\.env\.DEV && [^\n]*\{\s*extensions\.push\(\(await import\("\.\/dev-extensions"\)\)/);
    expect(main.match(/dev-extensions/g)?.length).toBe(1);
  });
});
