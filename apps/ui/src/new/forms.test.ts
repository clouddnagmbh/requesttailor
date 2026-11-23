// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand B: the forms of the "New…" dialog (defaults, checks, what they turn into).

import { describe, expect, it } from "vitest";

import type { CollectionEntry, Environment, RequestSpec, TreeNode } from "../commands/types";
import { sessionKind } from "../stream/kind";
import {
  folderChildren,
  folderName,
  folderOptions,
  initialForm,
  mcpTest,
  newCollection,
  newEnvironment,
  requestSpec,
  splitCommand,
  validate,
  withName,
  type CollectionForm,
  type FormContext,
  type RequestForm,
} from "./forms";
import { NEW_ITEMS } from "./kinds";

const entry = (id: string, root: string, dir: string): CollectionEntry => ({
  id,
  name: id,
  location: id,
  root,
  dir,
  repo: null,
  dirty: 0,
});

const ctx: FormContext = {
  collection: "c1",
  folder: "users",
  collections: [entry("c1", "r1", "billing"), entry("c2", "r1", "shop")],
  desktop: false,
};

const blank = (): RequestSpec => ({
  name: "Ping",
  seq: null,
  method: "GET",
  url: "",
  headers: [{ name: "accept", value: "application/json", enabled: true }],
  params: [],
  body: { type: "none" },
  auth: { type: "inherit" } as RequestSpec["auth"],
  vars: [],
  settings: { timeout_ms: null, follow_redirects: null, max_redirects: null, encode_url: null },
  scripts: [],
  assertions: [],
  actions: [],
});

const tree: TreeNode = {
  kind: "folder",
  name: "Demo",
  path: "",
  children: [
    { kind: "request", name: "Health", path: "health.yml", method: "GET" },
    {
      kind: "folder",
      name: "Users",
      path: "users",
      children: [{ kind: "folder", name: "Admin area", path: "users/admin", children: [] }],
    },
  ],
};

describe("defaults", () => {
  it("every kind starts with a name and the target the dialog was opened for", () => {
    for (const { kind } of NEW_ITEMS) {
      const form = initialForm(kind, ctx);
      expect(form.name.length, kind).toBeGreaterThan(0);
      // Only the MCP test has a field without a default: the server's address.
      expect(Object.keys(validate(form, { collections: ctx.collections })), kind).toEqual(kind === "mcp-test" ? ["url"] : []);
      if (form.kind !== "collection") expect(form.collection).toBe("c1");
    }
    expect(initialForm("http", ctx)).toMatchObject({ method: "GET", url: "", folder: "users" });
    expect(initialForm("folder", ctx)).toMatchObject({ folder: "users" });
    expect(initialForm("mcp-test", ctx)).toMatchObject({ transport: "http", tool: "" });
    expect(initialForm("environment", ctx)).toMatchObject({ copyFrom: null });
  });

  it("a collection goes into a subfolder of the active root, named after it, with a Local environment", () => {
    const form = initialForm("collection", ctx) as CollectionForm;
    expect(form).toMatchObject({ place: "root", root: "r1", dir: "new-collection", environment: "Local", baseUrl: "" });
    // No root yet: the folder picker ("Elsewhere…").
    expect(initialForm("collection", { ...ctx, collection: null, collections: [] })).toMatchObject({ place: "elsewhere", root: null });
  });

  it("without an open collection the forms say so", () => {
    const form = initialForm("http", { ...ctx, collection: null, collections: [] });
    expect(validate(form).collection).toMatch(/collection first/);
  });
});

describe("folder names", () => {
  it("come from the name", () => {
    expect(folderName("Payments API v2")).toBe("payments-api-v2");
    expect(folderName("  Über uns / Team ")).toBe("über-uns-team");
    expect(folderName("***")).toBe("");
    expect(folderName("a".repeat(80)).length).toBe(64);
  });

  it("follow the name until edited", () => {
    let form = initialForm("collection", ctx) as CollectionForm;
    form = withName(form, "Shop Backend");
    expect(form.dir).toBe("shop-backend");
    form = { ...form, dir: "apis/shop", dirEdited: true };
    form = withName(form, "Shop");
    expect(form).toMatchObject({ name: "Shop", dir: "apis/shop" });
  });
});

describe("checks", () => {
  it("need a name", () => {
    for (const { kind } of NEW_ITEMS) {
      expect(validate({ ...initialForm(kind, ctx), name: "  " }).name, kind).toBe("Give it a name.");
    }
  });

  it("refuse a folder that exists or a name with a slash", () => {
    const form = { ...initialForm("folder", ctx), folder: "" };
    const siblings = folderChildren(tree, "");
    expect(siblings).toEqual(["Users", "users"]);
    expect(validate({ ...form, name: "users" }, { siblings }).name).toMatch(/already a folder/);
    expect(validate({ ...form, name: "a/b" }, { siblings }).name).toMatch(/cannot contain/);
    expect(validate({ ...form, name: ".." }).name).toMatch(/cannot be called/);
    expect(validate({ ...form, name: "Orders" }, { siblings })).toEqual({});
    expect(folderChildren(tree, "users")).toEqual(["Admin area", "admin"]);
  });

  it("refuse an environment that exists (any case)", () => {
    const form = { ...initialForm("environment", ctx), name: "local" };
    expect(validate(form, { environments: ["Local", "PRD"] }).name).toMatch(/already an environment/);
    expect(validate({ ...form, name: "Staging" }, { environments: ["Local"] })).toEqual({});
  });

  it("refuse a collection folder that another collection uses, or a bad path", () => {
    const form = { ...(initialForm("collection", ctx) as CollectionForm), dir: "Billing" };
    expect(validate(form, { collections: ctx.collections }).dir).toMatch(/Another collection/);
    expect(validate({ ...form, dir: "../x" }).dir).toMatch(/folder name/);
    expect(validate({ ...form, dir: "" }).dir).toMatch(/Give the folder a name/);
    expect(validate({ ...form, dir: "apis/new" }, { collections: ctx.collections })).toEqual({});
    // Elsewhere: no folder to check here.
    expect(validate({ ...form, place: "elsewhere", root: null, dir: "" })).toEqual({});
  });

  it("an MCP test needs the server", () => {
    const form = initialForm("mcp-test", ctx);
    expect(validate(form).url).toMatch(/address/);
    expect(validate({ ...form, transport: "stdio" } as typeof form).command).toMatch(/command/);
  });
});

describe("what the forms create", () => {
  const req = (over: Partial<RequestForm>): RequestForm => ({ ...(initialForm("http", ctx) as RequestForm), ...over });

  it("HTTP: method and URL", () => {
    const spec = requestSpec(blank(), req({ method: "POST", url: " https://api.example.com/users " }));
    expect(spec).toMatchObject({ method: "POST", url: "https://api.example.com/users" });
    expect(sessionKind(spec)).toBeNull();
  });

  it("SSE: a GET asking for text/event-stream", () => {
    const spec = requestSpec(blank(), req({ kind: "sse", url: "https://x/events" }));
    expect(spec.method).toBe("GET");
    expect(spec.headers).toEqual([{ name: "Accept", value: "text/event-stream", enabled: true }]);
    expect(sessionKind(spec)).toBe("sse");
  });

  it("GraphQL, gRPC and WebSocket get their protocol", () => {
    expect(requestSpec(blank(), req({ kind: "graphql", method: "GET" }))).toMatchObject({ method: "POST", body: { type: "graphql" } });
    expect(sessionKind(requestSpec(blank(), req({ kind: "grpc" })))).toBe("grpc");
    const ws = requestSpec(blank(), req({ kind: "websocket", url: "wss://echo" }));
    expect(ws).toMatchObject({ method: "WS", url: "wss://echo" });
    expect(sessionKind(ws)).toBe("websocket");
  });

  it("an environment copies variables but never secret values", () => {
    const from: Environment = {
      name: "Local",
      variables: [
        { name: "baseUrl", value: "http://localhost", enabled: true, secret: false },
        { name: "token", value: "should-not-be-here", enabled: true, secret: true },
      ],
      client_certificates: [],
      external_secrets: null,
    };
    const env = newEnvironment(" Staging ", from);
    expect(env.name).toBe("Staging");
    expect(env.variables).toEqual([
      { name: "baseUrl", value: "http://localhost", enabled: true, secret: false },
      { name: "token", value: null, enabled: true, secret: true },
    ]);
    expect(from.variables[1].value).toBe("should-not-be-here");
    expect(newEnvironment("Empty", null)).toEqual({ name: "Empty", variables: [], client_certificates: [], external_secrets: null });
  });

  it("a collection: NewCollection with baseUrl and first environment", () => {
    const form = { ...(initialForm("collection", ctx) as CollectionForm), name: "Shop", dir: "shop-api/", baseUrl: " http://localhost:3000 " };
    expect(newCollection(form)).toEqual({ name: "Shop", root: "r1", dir: "shop-api", base_url: "http://localhost:3000", environment: "Local" });
    expect(newCollection({ ...form, place: "elsewhere", baseUrl: "", environment: " " })).toEqual({
      name: "Shop",
      root: null,
      dir: "",
      base_url: null,
      environment: null,
    });
  });

  it("an MCP test: the server, nothing called yet", () => {
    const form = { ...initialForm("mcp-test", ctx), name: "Everything" } as ReturnType<typeof initialForm> & { kind: "mcp-test" };
    expect(mcpTest({ ...form, url: "http://localhost:3001/mcp" })).toEqual({
      name: "Everything",
      server: { transport: "http", url: "http://localhost:3001/mcp", headers: [] },
      tool: "",
      arguments: {},
      assertions: [],
    });
    expect(mcpTest({ ...form, transport: "stdio", command: `npx -y "my server" --port 3`, tool: "echo" }).server).toEqual({
      transport: "stdio",
      command: "npx",
      args: ["-y", "my server", "--port", "3"],
      env: [],
      cwd: null,
    });
    expect(splitCommand(` a  'b c' "d" `)).toEqual(["a", "b c", "d"]);
  });

  it("lists the folders of a tree for the target picker", () => {
    expect(folderOptions(tree)).toEqual([
      { path: "", name: "", depth: 0 },
      { path: "users", name: "Users", depth: 1 },
      { path: "users/admin", name: "Admin area", depth: 2 },
    ]);
    expect(folderOptions(null)).toEqual([{ path: "", name: "", depth: 0 }]);
  });
});
