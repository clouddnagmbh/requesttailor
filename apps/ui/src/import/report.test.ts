// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { mockImports } from "../commands/mock-import";
import type { ImportPlan } from "../commands/types";
import { groupLosses, pasted, reportText, summary } from "./report";

const enc = (s: string) => new TextEncoder().encode(s);

const plan: ImportPlan = {
  id: 1,
  format: "postman",
  collection_name: "Shop",
  folder_name: "Shop",
  files: 4,
  report: {
    format: "postman",
    requests: 2,
    folders: 1,
    environments: 1,
    losses: [
      { location: "Users / Get", kind: "test", effect: "dropped", detail: "test script (2 lines) not imported" },
      { location: "Users", kind: "script", effect: "dropped", detail: "pre-request script (1 line) not imported" },
      { location: "Users / Login", kind: "auth", effect: "approximated", detail: "the password is stored ..." },
    ],
  },
};

describe("import report", () => {
  it("groups losses by kind in a fixed order and counts effects", () => {
    const groups = groupLosses(plan.report.losses);
    expect(groups.map((g) => [g.kind, g.dropped, g.approximated])).toEqual([
      ["script", 1, 0],
      ["test", 1, 0],
      ["auth", 0, 1],
    ]);
  });

  it("renders a copyable text report", () => {
    const text = reportText(plan, ["shop.json", "prod.json"]);
    expect(text).toContain('Import of shop.json, prod.json (Postman) as "Shop"');
    expect(text).toContain("2 requests, 1 folder, 1 environment");
    expect(text).toContain("Loss report: 3 (2 dropped, 1 approximated)");
    expect(text).toContain("  dropped       Users: pre-request script (1 line) not imported");
    expect(text).toContain("  approximated  Users / Login: the password is stored ...");
    const none = reportText({ ...plan, report: { ...plan.report, losses: [] } }, ["a"]);
    expect(none).toContain("Nothing was lost");
    expect(summary(plan)).toBe("2 requests, 1 folder, 1 environment");
  });

  it("names pasted text so the core recognises it", () => {
    expect(pasted("curl https://x").name).toBe("pasted.txt");
    expect(pasted("GET https://x\nAccept: */*").name).toBe("pasted.http");
    expect(pasted("### Login\nPOST https://x").name).toBe("pasted.http");
    expect(new TextDecoder().decode(pasted("curl x").data)).toBe("curl x");
  });
});

describe("mock importer", () => {
  it("detects, refuses invalid files and opens on write", async () => {
    let opened = 0;
    const api = mockImports(async () => {
      opened++;
      return { id: "mock-collection", name: "Demo API", location: "mock://demo" };
    });
    const p = await api.analyze([{ name: "shop.postman_collection.json", data: enc("{}") }], null);
    expect(p.format).toBe("postman");
    expect(p.collection_name).toBe("shop");
    await expect(api.analyze([{ name: "x.json", data: enc("INVALID") }], null)).rejects.toThrow("not valid JSON");
    await expect(api.analyze([{ name: "x.bin", data: enc("??") }], null)).rejects.toThrow("unknown file format");
    const info = await api.write(p.id);
    expect(opened).toBe(1);
    expect(info?.name).toBe("shop");
    await expect(api.write(p.id)).rejects.toThrow("not found");
  });
});
