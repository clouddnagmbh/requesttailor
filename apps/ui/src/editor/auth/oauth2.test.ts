// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { OAuth2Param } from "../../commands/types";
import { FRESH_OAUTH2 } from "./fresh";
import {
  addParam,
  collectionOnly,
  groupParams,
  removeParam,
  stepsFor,
  switchFlow,
  switchTokenPlacement,
  updateParam,
  type OAuth2Auth,
} from "./oauth2";

const fresh = FRESH_OAUTH2 as OAuth2Auth;
const param = (step: OAuth2Param["step"], name: string): OAuth2Param => ({ step, name, value: "", placement: "body" });

const code: OAuth2Auth = {
  ...fresh,
  flow: "authorization-code",
  authorization_url: "https://idp/auth",
  callback_url: "http://127.0.0.1:9876/cb",
  state: "{{state}}",
  pkce: "S256",
  access_token_url: "https://idp/token",
  refresh_token_url: "https://idp/refresh",
  client_id: "app",
  client_secret: "{{secret}}",
  scope: "openid",
  parameters: [param("authorization", "audience"), param("access-token", "resource"), param("refresh", "r")],
};

describe("switchFlow", () => {
  it("keeps the client, endpoints, scope and token settings", () => {
    const next = switchFlow({ ...code, flow: "client-credentials" } as OAuth2Auth, "password");
    expect(next).toMatchObject({ flow: "password", username: "", password: "", client_id: "app", scope: "openid" });
    expect(next.access_token_url).toBe("https://idp/token");
    expect(next.token).toEqual(fresh.token);
  });

  it("starts authorization code with PKCE S256", () => {
    const next = switchFlow(fresh, "authorization-code");
    expect(next).toMatchObject({ flow: "authorization-code", authorization_url: "", callback_url: "", state: "", pkce: "S256" });
  });

  it("carries the browser fields between authorization code and implicit", () => {
    const implicit = switchFlow(code, "implicit");
    expect(implicit).toMatchObject({
      flow: "implicit",
      authorization_url: "https://idp/auth",
      callback_url: "http://127.0.0.1:9876/cb",
      state: "{{state}}",
    });
    // Implicit has no token endpoint (the file has no place for it).
    expect(implicit.access_token_url).toBe("");
    expect(implicit.refresh_token_url).toBe("");
    expect("pkce" in implicit).toBe(false);
    const back = switchFlow(implicit, "authorization-code");
    expect(back).toMatchObject({ flow: "authorization-code", authorization_url: "https://idp/auth", pkce: "S256" });
  });

  it("drops the flow fields of the previous flow", () => {
    const next = switchFlow(code, "client-credentials");
    expect(next).not.toHaveProperty("authorization_url");
    expect(next).not.toHaveProperty("pkce");
    expect(next.flow).toBe("client-credentials");
  });

  it("drops parameters of steps the new flow does not have", () => {
    expect(switchFlow(code, "client-credentials").parameters.map((p) => p.name)).toEqual(["resource", "r"]);
    expect(switchFlow(code, "implicit").parameters.map((p) => p.name)).toEqual(["audience"]);
  });

  it("returns the same auth for the same flow", () => {
    expect(switchFlow(code, "authorization-code")).toBe(code);
  });
});

describe("flows", () => {
  it("knows which flows are collection defaults only", () => {
    expect(collectionOnly("device-code")).toBe(true);
    expect(collectionOnly("saml2-bearer")).toBe(true);
    expect(collectionOnly("authorization-code")).toBe(false);
  });

  it("offers the steps a flow has", () => {
    expect(stepsFor("authorization-code")).toEqual(["authorization", "access-token", "refresh"]);
    expect(stepsFor("implicit")).toEqual(["authorization"]);
    expect(stepsFor("client-credentials")).toEqual(["access-token", "refresh"]);
    expect(stepsFor("device-code")).toEqual(["access-token", "refresh"]);
  });
});

describe("parameters", () => {
  it("groups by step in file order and keeps the order within a step", () => {
    const list = [param("refresh", "a"), param("access-token", "b"), param("authorization", "c"), param("access-token", "d")];
    expect(groupParams(list).map((p) => p.name)).toEqual(["c", "b", "d", "a"]);
  });

  it("adds a parameter at the end of its step", () => {
    const list = [param("authorization", "a"), param("refresh", "z")];
    const next = addParam(list, "access-token");
    expect(next.map((p) => [p.step, p.name])).toEqual([
      ["authorization", "a"],
      ["access-token", ""],
      ["refresh", "z"],
    ]);
    expect(next[1].placement).toBe("body");
    expect(addParam([], "authorization")[0].placement).toBe("query");
  });

  it("moves a parameter whose step changes to the end of the new step", () => {
    const list = [param("authorization", "a"), param("access-token", "b"), param("refresh", "c")];
    const next = updateParam(list, 0, { step: "refresh" });
    expect(next.map((p) => p.name)).toEqual(["b", "c", "a"]);
    expect(list[0].step).toBe("authorization");
  });

  it("updates name, value and placement in place", () => {
    const list = [param("access-token", "a"), param("access-token", "b")];
    expect(updateParam(list, 1, { value: "v", placement: "header" })[1]).toEqual({
      step: "access-token",
      name: "b",
      value: "v",
      placement: "header",
    });
  });

  it("removes a parameter", () => {
    expect(removeParam([param("refresh", "a"), param("refresh", "b")], 0).map((p) => p.name)).toEqual(["b"]);
  });
});

describe("switchTokenPlacement", () => {
  it("switches between header and query with sensible defaults", () => {
    const header = fresh.token;
    const query = switchTokenPlacement(header, "query");
    expect(query).toEqual({ ...header, placement: { in: "query", name: "access_token" } });
    expect(switchTokenPlacement(query, "header").placement).toEqual({ in: "header", prefix: "Bearer" });
    expect(switchTokenPlacement(header, "header")).toBe(header);
  });
});
