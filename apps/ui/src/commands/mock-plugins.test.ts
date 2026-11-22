// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { MockBackend } from "./mock";
import { mockManifest, mockPluginFile } from "./mock-plugins";

const demo = mockManifest({
  id: "demo",
  name: "Demo",
  contributes: {
    prepare: true,
    response: true,
    tabs: [{ id: "req", title: "Demo", place: "request", content_types: [] }],
    importers: [{ id: "imp", label: "Demo files", extensions: [".demo"] }],
  },
});

describe("mock plugins", () => {
  it("installs, notes sends, renders tabs, imports and removes", async () => {
    const b = new MockBackend();
    await b.openCollection();
    await expect(b.plugins.inspect({ name: "x.wasm", data: new Uint8Array([0, 97]) })).rejects.toMatchObject({ kind: "plugin" });
    await b.plugins.install(mockPluginFile(demo));
    expect((await b.plugins.list()).map((p) => [p.manifest.id, p.enabled])).toEqual([["demo", true]]);

    const path = "users/create-user.yml";
    const sent = await b.send(path, { environment: null, vars: [] });
    expect(sent.report.notes?.map((n) => n.text)).toEqual(["Demo prepared the request", "Demo saw the response"]);

    const target = { request_path: path, response_id: null };
    const first = await b.plugins.renderTab("demo", "req", target, null);
    expect(first.view.blocks.map((x) => x.type)).toContain("form");
    const added = await b.plugins.action("demo", "req", "add-header", target, first.state, []);
    expect(added.update_request?.headers.at(-1)).toMatchObject({ name: "X-Mock-Plugin", value: "1.0.0" });

    const plan = await b.imports.analyzeWithPlugin([{ name: "a.demo", data: new Uint8Array() }], "demo", "imp");
    expect(plan).toMatchObject({ format: "plugin", collection_name: "Demo import" });

    await b.plugins.setEnabled("demo", false);
    expect((await b.send(path, { environment: null, vars: [] })).report.notes).toBeUndefined();
    await expect(b.plugins.renderTab("demo", "req", target, null)).rejects.toMatchObject({ kind: "plugin" });
    await b.plugins.remove("demo");
    expect(await b.plugins.list()).toEqual([]);
  });
});
