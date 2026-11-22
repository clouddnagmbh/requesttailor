// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { type CookieJars, type StoredCookie, parseJars, webCookies } from "./cookies";
import type { WebHost } from "./web-backend";

function cookie(name: string, extra: Partial<StoredCookie> = {}): StoredCookie {
  return {
    name,
    value: `${name}-value`,
    domain: "api.example.com",
    host_only: true,
    path: "/",
    expires: null,
    secure: false,
    http_only: false,
    same_site: null,
    created: 1,
    ...extra,
  };
}

function host(paired: { value: boolean }, id = "c1") {
  return {
    session: () => ({ id }) as unknown as ReturnType<WebHost["session"]>,
    companionCredentials: async () =>
      paired.value ? { url: "http://127.0.0.1:1", token: "t" } : null,
  } as Pick<WebHost, "session" | "companionCredentials">;
}

describe("web cookie jar", () => {
  it("hands the jar to the core only with the companion", async () => {
    const paired = { value: false };
    const api = webCookies(host(paired), () => 100);
    expect(await api.forSend("c1", "Local")).toBeUndefined();

    paired.value = true;
    const jar = await api.forSend("c1", "Local");
    expect(jar).toBeDefined();
    expect(parseJars(jar!.jars)).toEqual({ version: 1, scopes: {} });
  });

  it("keeps what the core saves, per collection and environment", async () => {
    const api = webCookies(host({ value: true }), () => 100);
    const saved: CookieJars = {
      version: 1,
      scopes: {
        Local: [cookie("z"), cookie("a", { http_only: true }), cookie("old", { expires: 50 })],
        Prod: [cookie("p")],
      },
    };
    (await api.forSend("c1", "Local"))!.save(JSON.stringify(saved));

    const local = await api.list("Local");
    expect(local.map((c) => c.name)).toEqual(["a", "z"]);
    expect(local[0]).toEqual({
      name: "a",
      value: "a-value",
      domain: "api.example.com",
      path: "/",
      expires: null,
      secure: false,
      http_only: true,
      same_site: null,
    });
    expect((await api.list("Prod")).length).toBe(1);
    expect(await api.list("")).toEqual([]);
    // The next send gets them back.
    expect(parseJars((await api.forSend("c1", "Local"))!.jars)).toEqual(saved);
    // Another collection has its own jar.
    expect(parseJars((await api.forSend("c2", "Local"))!.jars).scopes).toEqual({});

    await api.delete("Local", "api.example.com", "/", "z");
    expect((await api.list("Local")).map((c) => c.name)).toEqual(["a"]);
    await api.clear("Local");
    expect(await api.list("Local")).toEqual([]);
    expect((await api.list("Prod")).length).toBe(1);
  });

  it("reads anything that is not a jar as empty", () => {
    expect(parseJars("not json")).toEqual({ version: 1, scopes: {} });
    expect(parseJars('{"version":2,"scopes":{}}')).toEqual({ version: 1, scopes: {} });
  });
});
