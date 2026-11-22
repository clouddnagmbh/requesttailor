// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { MockBackend } from "./mock";
import { maskCookieHeader, maskSetCookie, mockCookies } from "./mock-cookies";

async function request(b: MockBackend, name: string, url: string): Promise<string> {
  const path = await b.createRequest("", name);
  const spec = await b.getRequest(path);
  await b.updateRequest(path, { ...spec, url });
  return path;
}

function headers(list: [string, string][] | undefined, name: string): string[] {
  return (list ?? []).filter(([n]) => n.toLowerCase() === name).map(([, v]) => v);
}

describe("mock cookie jar", () => {
  it("stores Set-Cookie per environment, sends it next time, masks values", async () => {
    const b = new MockBackend();
    await b.openCollection();
    const login = await request(b, "Login", "https://api.example.com/cookies/set?session=abc&theme=dark");
    const next = await request(b, "Next", "https://api.example.com/users");

    const first = await b.send(login, { environment: "Local", vars: [] });
    expect(headers(first.report.response?.headers, "set-cookie")).toEqual(["session=***; Path=/", "theme=***; Path=/"]);
    const listed = await b.cookies.list("Local");
    expect(listed.map((c) => [c.name, c.value, c.domain, c.path])).toEqual([
      ["session", "abc", "api.example.com", "/"],
      ["theme", "dark", "api.example.com", "/"],
    ]);

    const second = await b.send(next, { environment: "Local", vars: [] });
    expect(headers(second.report.request?.headers, "cookie")).toEqual(["session=***; theme=***"]);
    expect(JSON.stringify(second.report)).not.toContain("abc");

    const prd = await b.send(next, { environment: "PRD", vars: [] });
    expect(headers(prd.report.request?.headers, "cookie")).toEqual([]);
    expect(await b.cookies.list("PRD")).toEqual([]);

    await b.cookies.delete("Local", "api.example.com", "/", "session");
    expect((await b.cookies.list("Local")).map((c) => c.name)).toEqual(["theme"]);
    await b.cookies.clear("Local");
    expect(await b.cookies.list("Local")).toEqual([]);
  });

  it("matches host, path, Secure and expiry like the core", () => {
    let now = 1_000;
    const jar = mockCookies(() => now);
    jar.store("", "https://a.example.com/api/x", [
      "p=1; Path=/api",
      "s=2; Path=/; Secure; HttpOnly; SameSite=Lax",
      "t=3; Path=/; Max-Age=10",
    ]);
    expect(jar.header("", "https://a.example.com/api/y")).toBe("p=1; s=2; t=3");
    expect(jar.header("", "http://a.example.com/apix")).toBe("t=3");
    expect(jar.header("", "https://b.example.com/api")).toBeNull();
    now = 20_000;
    expect(jar.header("", "https://a.example.com/")).toBe("s=2");
    jar.store("", "https://a.example.com/", ["s=; Max-Age=0"]);
    expect(jar.header("", "https://a.example.com/")).toBeNull();
  });

  it("masks like the core's report", () => {
    expect(maskCookieHeader("a=1; b=two")).toBe("a=***; b=***");
    expect(maskSetCookie("sid=abc; Path=/; HttpOnly")).toBe("sid=***; Path=/; HttpOnly");
  });
});
