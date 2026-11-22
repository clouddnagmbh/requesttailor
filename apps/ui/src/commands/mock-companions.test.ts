// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { MockBackend } from "./mock";

describe("mock companions", () => {
  it("stages companion files until save, refuses collection files", async () => {
    const b = new MockBackend();
    await b.openCollection();
    await b.companions.stage("orders/metadata.xml", "<edmx/>");
    expect(await b.companions.read("orders/metadata.xml")).toBe("<edmx/>");
    expect(await b.companions.list("orders/")).toEqual(["orders/metadata.xml"]);
    expect(await b.dirty()).toContain("orders/metadata.xml");
    await expect(b.companions.stage("orders/folder.yml", "x")).rejects.toThrow(/cannot write/);
    await expect(b.companions.stage("../x.xml", "x")).rejects.toThrow(/cannot write/);
    const saved = await b.save();
    expect(saved).toContainEqual({ path: "orders/metadata.xml", text: "<edmx/>", deleted: false });
    expect(await b.dirty()).not.toContain("orders/metadata.xml");
    expect(await b.companions.read("missing.xml")).toBeNull();
  });
});
