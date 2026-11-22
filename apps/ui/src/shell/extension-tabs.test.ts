// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { MockBackend } from "../commands/mock";
import type { AppExtension, ExtensionTab } from "../extensions";
import { AppState } from "../state/app.svelte";
import { extensionTabsFor, type ProvidedExtensions } from "./extension-tabs";

// The component is never rendered here.
const component = (() => {}) as unknown as ExtensionTab["component"];

function provided(extensions: AppExtension[]): ProvidedExtensions {
  const backend = new MockBackend();
  return { extensions, ctx: { backend, app: new AppState(backend) } };
}

describe("extension tabs", () => {
  const one: AppExtension = {
    id: "one",
    tabs: () => [
      { id: "query", title: "Query", place: "request", component },
      { id: "table", title: "Table", place: "response", contentTypes: ["application/json"], component },
      { id: "any", title: "Any", place: "response", component },
      { id: "graph", title: "Graph", place: "collection", component },
    ],
  };
  const two: AppExtension = { id: "two", tabs: () => [{ id: "graph", title: "Graph 2", place: "collection", component }] };

  it("lists the tabs of a place in extension order with unique keys", () => {
    const p = provided([one, { id: "none" }, two]);
    expect(extensionTabsFor(p, "collection").map((t) => t.key)).toEqual(["one/graph", "two/graph"]);
    expect(extensionTabsFor(p, "request").map((t) => [t.key, t.extension])).toEqual([["one/query", "one"]]);
  });

  it("filters response tabs by content type", () => {
    const p = provided([one]);
    expect(extensionTabsFor(p, "response", "application/json; charset=utf-8").map((t) => t.tab.id)).toEqual(["table", "any"]);
    expect(extensionTabsFor(p, "response", "application/xml").map((t) => t.tab.id)).toEqual(["any"]);
    expect(extensionTabsFor(p, "response", null).map((t) => t.tab.id)).toEqual(["any"]);
  });

  it("shows none without a provider or when an extension fails", () => {
    expect(extensionTabsFor(null, "request")).toEqual([]);
    const broken: AppExtension = {
      id: "broken",
      tabs: () => {
        throw new Error("no");
      },
    };
    expect(extensionTabsFor(provided([broken, two]), "collection").map((t) => t.key)).toEqual(["two/graph"]);
  });

  it("asks the extension each time, so tabs follow its state", () => {
    let allowed = false;
    const gated: AppExtension = {
      id: "gated",
      tabs: () => (allowed ? [{ id: "x", title: "X", place: "request", component }] : []),
    };
    const p = provided([gated]);
    expect(extensionTabsFor(p, "request")).toEqual([]);
    allowed = true;
    expect(extensionTabsFor(p, "request").map((t) => t.key)).toEqual(["gated/x"]);
  });
});
