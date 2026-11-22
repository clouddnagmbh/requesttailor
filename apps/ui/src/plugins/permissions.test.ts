// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { mockManifest } from "../commands/mock-plugins";
import type { PluginInfo } from "../commands/types";
import {
  addedPermissions,
  contentTypeOf,
  describePermissions,
  installPlan,
  matchesContentType,
  pluginImporters,
  tabsFor,
} from "./permissions";

const none = { network: [], storage: false, variables: false };

describe("plugin permissions", () => {
  it("describes each permission", () => {
    expect(describePermissions(none)).toEqual([]);
    expect(
      describePermissions({ network: ["*", "{request}", "*.example.com", "api.test:8443"], storage: true, variables: true }),
    ).toEqual([
      "Network: any host",
      "Network: the host of the request being sent",
      "Network: subdomains of example.com",
      "Network: api.test:8443",
      "Storage: keep its own data on this device",
      "Variables: read non-secret variables of the request",
    ]);
  });

  it("lists only what an update adds", () => {
    const granted = { network: ["api.test"], storage: true, variables: false };
    expect(addedPermissions(granted, granted)).toEqual([]);
    expect(addedPermissions(granted, { ...granted, network: ["API.test"] })).toEqual([]);
    expect(addedPermissions(granted, none)).toEqual([]);
    expect(addedPermissions(granted, { network: ["api.test", "{request}"], storage: true, variables: true })).toEqual([
      "Network: the host of the request being sent",
      "Variables: read non-secret variables of the request",
    ]);
  });

  it("tells a new install from an update", () => {
    const v1 = mockManifest({ id: "p", permissions: { network: [], storage: true, variables: false } });
    const installed: PluginInfo[] = [{ manifest: v1, granted: true, enabled: true }];
    expect(installPlan(mockManifest({ id: "q" }), installed)).toEqual({ kind: "new", permissions: [] });
    const v2 = mockManifest({
      id: "p",
      version: "2.0.0",
      permissions: { network: ["{request}"], storage: true, variables: false },
    });
    expect(installPlan(v2, installed)).toEqual({
      kind: "update",
      from: "1.0.0",
      to: "2.0.0",
      added: ["Network: the host of the request being sent"],
    });
  });
});

describe("plugin tabs", () => {
  it("matches content types", () => {
    expect(matchesContentType([], null)).toBe(true);
    expect(matchesContentType(["application/json"], "application/json; charset=utf-8")).toBe(true);
    expect(matchesContentType(["application/json"], "application/problem+json")).toBe(true);
    expect(matchesContentType(["application/json"], "text/plain")).toBe(false);
    expect(matchesContentType(["application/json"], null)).toBe(false);
    expect(matchesContentType(["text/*"], "text/html")).toBe(true);
    expect(matchesContentType(["*/*"], null)).toBe(true);
    expect(contentTypeOf([["Content-Type", "text/xml"]])).toBe("text/xml");
    expect(contentTypeOf([])).toBeNull();
  });

  it("collects tabs and importers of enabled plugins in order", () => {
    const plugin = (id: string, enabled: boolean): PluginInfo => ({
      enabled,
      granted: true,
      manifest: mockManifest({
        id,
        contributes: {
          welcome: null,
          prepare: false,
          response: false,
          tabs: [
            { id: "req", title: "Req", place: "request", content_types: [] },
            { id: "json", title: "JSON", place: "response", content_types: ["application/json"] },
            // Collection tabs (wave 6) ignore content types.
            { id: "all", title: "All", place: "collection", content_types: ["application/json"] },
          ],
          importers: [{ id: "imp", label: "Thing", extensions: [] }],
        },
      }),
    });
    const list = [plugin("a", true), plugin("b", false), plugin("c", true)];
    expect(tabsFor(list, "request").map((t) => t.key)).toEqual(["a/req", "c/req"]);
    expect(tabsFor(list, "response", "application/json").map((t) => t.key)).toEqual(["a/json", "c/json"]);
    expect(tabsFor(list, "response", "text/html")).toEqual([]);
    expect(tabsFor(list, "collection").map((t) => t.key)).toEqual(["a/all", "c/all"]);
    expect(pluginImporters(list).map((i) => i.value)).toEqual(["a/imp", "c/imp"]);
  });
});
