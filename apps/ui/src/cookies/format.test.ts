// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { CookieInfo } from "../commands/types";
import { cookieFlags, cookieKey, formatExpires, sortCookies } from "./format";

const cookie = (over: Partial<CookieInfo>): CookieInfo => ({
  name: "sid",
  value: "abc",
  domain: "example.com",
  path: "/",
  expires: null,
  secure: false,
  http_only: false,
  same_site: null,
  ...over,
});

describe("cookie formatting", () => {
  it("shows session cookies, dates and expired ones", () => {
    expect(formatExpires(null)).toBe("Session");
    const at = new Date(2030, 0, 2, 3, 4).getTime();
    expect(formatExpires(at, at - 1)).toBe("2030-01-02 03:04");
    expect(formatExpires(at, at + 1)).toBe("2030-01-02 03:04 (expired)");
  });

  it("lists flags", () => {
    expect(cookieFlags(cookie({}))).toEqual([]);
    expect(cookieFlags(cookie({ secure: true, http_only: true, same_site: "Lax" }))).toEqual([
      "Secure",
      "HttpOnly",
      "SameSite=Lax",
    ]);
  });

  it("sorts by domain, path, name and keys by all three", () => {
    const list = [
      cookie({ domain: "b.com", name: "a" }),
      cookie({ domain: "a.com", path: "/x", name: "z" }),
      cookie({ domain: "a.com", path: "/", name: "y" }),
    ];
    expect(sortCookies(list).map((c) => `${c.domain}${c.path}${c.name}`)).toEqual(["a.com/y", "a.com/xz", "b.com/a"]);
    expect(list[0].domain).toBe("b.com");
    expect(cookieKey(cookie({}))).not.toBe(cookieKey(cookie({ path: "/a" })));
  });
});
