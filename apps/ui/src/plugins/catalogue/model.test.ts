// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { CatalogueEntry, SourceIndex } from "../../commands/types";
import { claimedRights, commitNote, explain, looksBlocked, rows, shortSum, sourceLabel } from "./model";

const entry = (id: string, name: string, description = ""): CatalogueEntry => ({
  id,
  name,
  version: "1.0.0",
  api: 1,
  description,
  wasm: { url: `${id}.wasm`, sha256: "a".repeat(64), size: 1 },
  permissions: { network: ["api.example.test"], storage: true, variables: false },
});

const index = (url: string, ...entries: CatalogueEntry[]): SourceIndex => ({
  source: { url, commit: null, name: "Idx" },
  index: { version: 1, name: "Idx", plugins: entries },
});

describe("catalogue model", () => {
  it("names a source by its index, else by the address", () => {
    expect(sourceLabel({ url: "https://x.test/p.json", name: "Team index" })).toBe("Team index");
    expect(sourceLabel({ url: "https://x.test/p.json", name: null })).toBe("https://x.test/p.json");
    expect(sourceLabel({ url: "https://x.test/p.json", name: "  " })).toBe("https://x.test/p.json");
  });

  it("shows the commit a Git source was read at", () => {
    expect(commitNote({ url: "u", commit: "3fa1c9e0000000" })).toBe("index at 3fa1c9e");
    expect(commitNote({ url: "u", commit: null })).toBeNull();
  });

  it("searches id, name and description with every word", () => {
    const idx = [index("https://x.test/p.json", entry("json-tools", "JSON tools", "Pretty-print bodies"), entry("acme-lint", "Acme lint"))];
    expect(rows(idx, [], [], "").length).toBe(2);
    expect(rows(idx, [], [], "json PRETTY").map((r) => r.entry.id)).toEqual(["json-tools"]);
    expect(rows(idx, [], [], "nothing").length).toBe(0);
  });

  it("knows what is installed and what has an update", () => {
    const idx = [index("https://x.test/p.json", entry("acme-lint", "Acme lint"))];
    const installed = [{ plugin: "acme-lint", source: "https://x.test/p.json", version: "0.9.0", sha256: "b".repeat(64) }];
    const updates = [{ plugin: "acme-lint", source: "https://x.test/p.json", installed: "0.9.0", available: "1.0.0" }];
    const [r] = rows(idx, installed, updates, "");
    expect(r?.installed).toBe("0.9.0");
    expect(r?.update).toBe(true);
    // An update known from another source is not this source's.
    expect(rows(idx, installed, [{ ...updates[0]!, source: "https://other.test/p.json" }], "")[0]?.update).toBe(false);
  });

  it("describes the rights the index claims", () => {
    expect(claimedRights(entry("a", "A")).join("|")).toContain("Network: api.example.test");
  });

  it("shortens a checksum", () => {
    expect(shortSum("a".repeat(64))).toBe("aaaaaaaa…aaaa");
    expect(shortSum("abc")).toBe("abc");
  });

  it("explains a blocked read but leaves an HTTP status alone", () => {
    expect(looksBlocked("Failed to fetch")).toBe(true);
    expect(looksBlocked("HTTP 404")).toBe(false);
    expect(explain("TypeError: Failed to fetch")).toContain("companion");
    expect(explain("HTTP 404")).toBe("HTTP 404");
  });
});
