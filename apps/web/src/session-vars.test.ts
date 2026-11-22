// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { SendReport, VarChange } from "@requesttailor/ui";

import { SessionStore, webSessionVars } from "./session-vars";
import type { Session } from "./web-backend";

const change = (name: string, value: string | null, scope: VarChange["scope"] = "runtime"): VarChange => ({
  name,
  value,
  scope,
});

function report(environment: string | null, var_changes: VarChange[]): SendReport {
  return {
    version: 1,
    request_path: "a.yml",
    environment,
    request: null,
    response: null,
    variables: [],
    unresolved: [],
    error: null,
    var_changes,
  };
}

describe("SessionStore", () => {
  it("keeps a send's changes per collection and environment", () => {
    const s = new SessionStore();
    s.keep("c1", report("Local", [change("token", "abc"), change("id", "7", "environment")]));
    s.keep("c1", report("Prod", [change("token", "prod")]));
    s.keep("c2", report("Local", [change("token", "other")]));
    expect(s.list("c1", "Local")).toEqual([change("token", "abc"), change("id", "7", "environment")]);
    expect(s.list("c1", "Prod")).toEqual([change("token", "prod")]);
    expect(s.list("c2", "Local")).toEqual([change("token", "other")]);
    expect(s.list("c1", null)).toEqual([]);
  });

  it("replaces, moves to the end and unsets", () => {
    const s = new SessionStore();
    s.apply("c", "", change("a", "1"));
    s.apply("c", "", change("b", "2"));
    s.apply("c", null, change("a", "3"));
    expect(s.list("c", "")).toEqual([change("b", "2"), change("a", "3")]);
    s.apply("c", "", change("b", null));
    expect(s.list("c", "")).toEqual([change("a", "3")]);
    s.clear("c", "");
    expect(s.list("c", "")).toEqual([]);
  });

  it("puts the session's variables before the caller's own, which win", () => {
    const s = new SessionStore();
    s.keep("c", report("Local", [change("token", "abc"), change("id", "7")]));
    expect(s.runtime("c", "Local", [["token", "typed"]])).toEqual([
      ["token", "abc"],
      ["id", "7"],
      ["token", "typed"],
    ]);
    expect(s.runtime("c", "Prod", [])).toEqual([]);
  });

  it("ignores reports without changes", () => {
    const s = new SessionStore();
    s.keep("c", { ...report("Local", []), var_changes: undefined });
    expect(s.list("c", "Local")).toEqual([]);
  });
});

describe("webSessionVars", () => {
  it("works on the open collection and is empty without one", async () => {
    const store = new SessionStore();
    let open: string | null = null;
    const api = webSessionVars(
      {
        session: () => {
          if (open === null) throw new Error("No collection is open.");
          return { id: open } as Session;
        },
      },
      store,
    );
    await api.setSessionVar("Local", change("a", "1"));
    expect(await api.sessionVars("Local")).toEqual([]);
    open = "c1";
    await api.setSessionVar("Local", change("a", "1"));
    expect(await api.sessionVars("Local")).toEqual([change("a", "1")]);
    expect(store.list("c1", "Local")).toEqual([change("a", "1")]);
    open = "c2";
    expect(await api.sessionVars("Local")).toEqual([]);
    open = "c1";
    await api.clearSessionVars("Local");
    expect(await api.sessionVars("Local")).toEqual([]);
  });
});
