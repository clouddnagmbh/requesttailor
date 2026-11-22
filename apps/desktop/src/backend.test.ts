// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { readFileSync } from "node:fs";

import { BackendError } from "@requesttailor/ui";
import { describe, expect, it } from "vitest";

import { COMMANDS, TauriBackend, type Command, type Invoke } from "./backend";

function fake(answer: (cmd: Command, args?: Record<string, unknown>) => unknown) {
  const calls: [Command, Record<string, unknown> | undefined][] = [];
  const invoke: Invoke = async (cmd, args) => {
    calls.push([cmd, args]);
    return answer(cmd, args);
  };
  return { backend: new TauriBackend(invoke), calls };
}

describe("TauriBackend", () => {
  it("maps every Backend method to its command with camelCase arguments", async () => {
    const { backend: b, calls } = fake(() => null);
    const spec = { name: "x" } as never;
    const env = { name: "Local", variables: [] };
    await b.openCollection();
    await b.tree();
    await b.getRequest("a.yml");
    await b.createRequest("users", "New");
    await b.updateRequest("a.yml", spec);
    await b.dirty();
    await b.save();
    await b.environments();
    await b.updateEnvironment(env);
    await b.deleteEnvironment("Local");
    await b.preview("a.yml", { environment: "Local", vars: [["v", "1"]] });
    await b.send("a.yml", { environment: null, vars: [] });
    await b.responseLines(1, 2, 3);
    await b.responseBytes(1, 0, 4).catch(() => undefined);
    await b.filterJson(1, "$.a");
    await b.releaseResponse(1);
    await b.secrets.has("Local", "token");
    await b.secrets.get("Local", "token");
    await b.secrets.set("Local", "token", "v");
    await b.secrets.delete("Local", "token");
    await b.recentCollections();
    await b.openRecent("/a", false);
    await b.search("users", 10);
    const g = b.git;
    await g.status();
    await g.diff("a.yml");
    await g.commit("msg", ["a.yml"]);
    await g.log(5);
    await g.branches();
    await g.checkout("main");
    await g.createBranch("x", true);
    await g.push();
    await g.pull();
    await g.conflict("a.yml");
    await g.resolve("a.yml", "ours");
    await g.init();
    await g.clone("https://example.com/r.git");
    await g.identity();
    await g.setIdentity({ name: "Ada", email: "a@example.com" });
    await g.hasCredentials("example.com");
    await g.setCredentials("example.com", "ada", "pw");
    await g.deleteCredentials("example.com");
    await b.imports.analyze("c.json", new Uint8Array([1, 2]), "postman");
    await b.imports.write(1);
    await b.imports.discard(1);
    await b.history.list("q", 20);
    await b.history.clear();
    expect(calls).toEqual([
      ["open_collection", undefined],
      ["tree", undefined],
      ["get_request", { path: "a.yml" }],
      ["create_request", { folder: "users", name: "New" }],
      ["update_request", { path: "a.yml", spec }],
      ["dirty", undefined],
      ["save", undefined],
      ["environments", undefined],
      ["update_environment", { env }],
      ["delete_environment", { name: "Local" }],
      ["preview", { path: "a.yml", input: { environment: "Local", vars: [["v", "1"]] } }],
      ["send", { path: "a.yml", input: { environment: null, vars: [] } }],
      ["response_lines", { id: 1, start: 2, count: 3 }],
      ["response_bytes", { id: 1, offset: 0, length: 4 }],
      ["filter_json", { id: 1, expression: "$.a" }],
      ["release_response", { id: 1 }],
      ["secret_has", { environment: "Local", variable: "token" }],
      ["secret_get", { environment: "Local", variable: "token" }],
      ["secret_set", { environment: "Local", variable: "token", value: "v" }],
      ["secret_delete", { environment: "Local", variable: "token" }],
      ["recent_collections", undefined],
      ["open_recent", { id: "/a", interactive: false }],
      ["search", { query: "users", limit: 10 }],
      ["git_status", undefined],
      ["git_diff", { path: "a.yml" }],
      ["git_commit", { message: "msg", paths: ["a.yml"] }],
      ["git_log", { limit: 5 }],
      ["git_branches", undefined],
      ["git_checkout", { branch: "main" }],
      ["git_create_branch", { name: "x", checkout: true }],
      ["git_push", undefined],
      ["git_pull", undefined],
      ["git_conflict", { path: "a.yml" }],
      ["git_resolve", { path: "a.yml", resolution: "ours" }],
      ["git_init", undefined],
      ["git_clone", { url: "https://example.com/r.git" }],
      ["git_identity", undefined],
      ["git_set_identity", { identity: { name: "Ada", email: "a@example.com" } }],
      ["git_has_credentials", { host: "example.com" }],
      ["git_set_credentials", { host: "example.com", username: "ada", password: "pw" }],
      ["git_delete_credentials", { host: "example.com" }],
      ["import_analyze", { fileName: "c.json", data: [1, 2], format: "postman" }],
      ["import_write", { id: 1 }],
      ["import_discard", { id: 1 }],
      ["history_list", { query: "q", limit: 20 }],
      ["history_clear", undefined],
    ]);
    expect(new Set(calls.map(([c]) => c))).toEqual(new Set(COMMANDS));
  });

  it("never forwards secret values from the webview at send", async () => {
    const { backend: b, calls } = fake(() => null);
    const input = { environment: "Local", vars: [], secrets: [["token", "leak"]] };
    await b.send("a.yml", input as never);
    expect(JSON.stringify(calls)).not.toContain("leak");
  });

  it("rethrows the serialized AppError as BackendError", async () => {
    const { backend: b } = fake(() => {
      throw { kind: "request-not-found", message: "a.yml" };
    });
    const err = await b.getRequest("a.yml").catch((e: unknown) => e);
    expect(err).toBeInstanceOf(BackendError);
    expect(err).toMatchObject({ kind: "request-not-found", message: "a.yml" });

    const { backend: c } = fake(() => {
      throw { kind: "response-not-found", message: 7 };
    });
    expect(await c.responseLines(7, 0, 1).catch((e: unknown) => e)).toMatchObject({
      kind: "response-not-found",
      message: "7",
    });

    const { backend: d } = fake(() => {
      throw "Command send not allowed by ACL";
    });
    expect(await d.send("a.yml", { environment: null, vars: [] }).catch((e: unknown) => e)).toMatchObject({
      kind: "shell",
      message: "Command send not allowed by ACL",
    });
  });

  it("returns response bytes as Uint8Array", async () => {
    const { backend: b } = fake(() => new Uint8Array([1, 2, 3]).buffer);
    const bytes = await b.responseBytes(1, 0, 3);
    expect(bytes).toBeInstanceOf(Uint8Array);
    expect([...bytes]).toEqual([1, 2, 3]);
  });

  it("is always native and offers no companion", async () => {
    const { backend: b } = fake(() => null);
    expect(b.kind).toBe("tauri");
    expect(b.transport.mode()).toBe("native");
    expect((await b.transport.companion()).available).toBe(false);
    await expect(b.transport.useCompanion(false)).resolves.toBeUndefined();
    await expect(b.transport.useCompanion(true)).rejects.toBeInstanceOf(BackendError);
  });

  it("calls only commands the capability allows", () => {
    const capability = JSON.parse(
      readFileSync(new URL("../src-tauri/capabilities/default.json", import.meta.url), "utf8"),
    ) as { permissions: string[] };
    const allowed = capability.permissions.map((p) => p.replace(/^allow-/, "").replaceAll("-", "_"));
    expect(allowed.sort()).toEqual([...COMMANDS].sort());
    const buildRs = readFileSync(new URL("../src-tauri/build.rs", import.meta.url), "utf8");
    const libRs = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    for (const c of COMMANDS) {
      expect(buildRs).toContain(`"${c}"`);
      expect(libRs).toMatch(new RegExp(`\\b\\w+::${c},`));
    }
  });
});
