// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { readFileSync } from "node:fs";

import { BackendError, type OpenApiOptions } from "@requesttailor/ui";
import { describe, expect, it } from "vitest";

import { COMMANDS, TauriBackend, type Command, type Invoke } from "./backend";
import { packImport, packPluginImport } from "./import";
import { packSpec } from "./openapi";

function fake(answer: (cmd: Command, args?: Record<string, unknown>) => unknown) {
  const calls: [Command, Record<string, unknown> | undefined][] = [];
  const invoke: Invoke = async (cmd, args) => {
    calls.push([cmd, args]);
    return answer(cmd, args);
  };
  return { backend: new TauriBackend(invoke, () => "channel", () => "stream-channel", () => "step-channel", () => "progress-channel", () => "ai-channel"), calls };
}

describe("TauriBackend", () => {
  it("maps every Backend method to its command with camelCase arguments", async () => {
    const { backend: b, calls } = fake(() => null);
    const spec = { name: "x" } as never;
    const env = { name: "Local", variables: [], client_certificates: [], external_secrets: null };
    await b.openCollection();
    await b.tree();
    await b.getRequest("a.yml");
    await b.createRequest("users", "New");
    await b.createFolder("Sales/Items");
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
    await g.pickRepository?.();
    await g.identity();
    await g.setIdentity({ name: "Ada", email: "a@example.com" });
    await g.hasCredentials("example.com");
    await g.setCredentials("example.com", "ada", "pw");
    await g.deleteCredentials("example.com");
    await g.scan(["a.yml"]);
    await g.revParse("HEAD~1");
    await g.treeAt("main");
    await g.show("HEAD", "/openapi.yaml");
    await g.fileLog("a.yml", 20);
    await g.mergeBase("main", "feature");
    await g.commits(null, "HEAD", 50);
    // Wave 14 (interface): graph and wizards.
    await g.graph({ limit: 2000 });
    await g.refs();
    await g.changes(null, "HEAD");
    await g.fileDiff("HEAD~1", null, "/README.md");
    await g.remotes();
    await g.stashes();
    await g.plan({ action: "create-branch", name: "x", start: "HEAD" });
    await g.apply({ action: "push" });
    await g.lastAction();
    await g.undo("1");
    // Wave 12, strand F: the contract.
    await b.contract.spec();
    await b.contract.check("HEAD~1");
    await b.contract.changes("v1", "v2");
    await b.contract.changelog("v1", "v2");
    await b.contract.changelogFacts("v1", "v2");
    // OpenAPI wizard.
    const specFiles = [{ name: "openapi.json", data: new Uint8Array([123, 125]) }];
    const options: OpenApiOptions = {
      operations: ["GET /pets"],
      grouping: "tag",
      naming: "summary",
      examples: "spec",
      servers: null,
      default_server: null,
      collection_name: null,
      keep_spec: true,
    };
    await b.openapi.fetch("https://api.example.com/openapi.json");
    await b.openapi.inspect(specFiles);
    await b.openapi.plan(specFiles, options);
    await b.openapi.requirements("pets/get.yml", { environment: "Dev", vars: [["id", "1"]] });
    await b.openapi.validateResponse("pets/get.yml", 4);
    await b.imports.analyze([{ name: "c.json", data: new Uint8Array([1, 2]) }], "postman");
    await b.imports.write(1);
    await b.imports.discard(1);
    await b.imports.writeInto(2, "Shop");
    await b.collections.current();
    await b.collections.add();
    await b.collections.removeRoot("/r");
    await b.collections.activate("/r/a");
    await b.collections.tree("/r/a");
    await b.collections.dirty("/r/a");
    await b.collections.saveAll();
    await b.collections.search("users", 20);
    await b.collections.create({ name: "N", root: "/r", dir: "n", base_url: null, environment: "Local" });
    await b.collections.reveal!("/r/a");
    await b.companions.list("orders");
    await b.companions.read("orders/metadata.xml");
    await b.companions.stage("orders/metadata.xml", "<edmx/>");
    await b.imports.adopt({ collection_name: "T", files: [], report: { format: "plugin", requests: 0, folders: 0, environments: 0, losses: [] } });
    await b.history.list("q", 20);
    await b.history.clear();
    const wasm = { name: "p.wasm", data: new Uint8Array([0, 97, 115, 109]) };
    const target = { request_path: "a.yml", response_id: 3 };
    await b.imports.analyzeWithPlugin([wasm], "odata", "edmx");
    await b.plugins.list();
    await b.plugins.inspect(wasm);
    await b.plugins.install(wasm);
    await b.plugins.remove("odata");
    await b.plugins.setEnabled("odata", false);
    await b.plugins.renderTab("odata", "query", target, null, "Local");
    await b.plugins.action("odata", "query", "run", target, { a: 1 }, [["top", "5"]]);
    await b.plugins.welcome();
    await b.plugins.chooseWelcome(["odata"]);
    await b.auth.tokens();
    await b.auth.fetchToken("a.yml", { environment: "Local", vars: [] });
    await b.auth.clearTokens(null);
    await b.auth.cancelPrompt();
    await b.cookies.list("Local");
    await b.cookies.delete("Local", "example.com", "/", "sid");
    await b.cookies.clear("Local");
    await b.scripts.trust();
    await b.scripts.setTrusted(true);
    await b.scripts.sessionVars("Local");
    await b.scripts.setSessionVar("Local", { name: "id", value: "7", scope: "runtime" });
    await b.scripts.clearSessionVars("Local");
    await b.runner.plan(null);
    await b.runner.parseData({ name: "d.csv", data: new Uint8Array([97]) });
    const plan = { requests: ["a.yml"], data: [], iterations: 1, delay_ms: 0, bail: false };
    await b.runner.run(plan, { environment: "Local", vars: [] }, () => {});
    await b.runner.cancel();
    await b.ai.config();
    await b.ai.setConfig({ provider: "anthropic", model: "claude-opus-5-5", base_url: null });
    await b.ai.hasKey("anthropic");
    await b.ai.setKey("anthropic", "sk-1");
    await b.ai.deleteKey("anthropic");
    await b.ai.ask({ task: "explain-error", path: "a.yml", response: 1 }, { environment: "Local", vars: [] });
    const settings = { profiles: [], roles: {}, preview: "first-use" as const };
    await b.ai.settings();
    await b.ai.setSettings(settings);
    await b.ai.hasProfileKey("default");
    await b.ai.setProfileKey("default", "sk-2");
    await b.ai.deleteProfileKey("default");
    await b.ai.policy();
    await b.ai.setDeviceOptOut(true);
    const call = { id: "c1", purpose: "x", system: "", messages: [], scope: {} };
    await b.ai.preview({ kind: "request", request: call }, { environment: "Local", vars: [] });
    await b.ai.confirmPreview("default");
    await b.ai.complete(call, { environment: "Local", vars: [] });
    await b.ai.cancel("c1");
    await b.agents.policy();
    await b.agents.setPolicy({ access: "read", environments: [] });
    await b.agents.pending();
    await b.agents.decide("abc", "denied");
    await b.agents.audit(10);
    await b.agents.check("orders/create.yml", "PRD");
    // Wave 8, strand B: secret values never go to the Rust side from the webview.
    const local = { environment: "Local", vars: [] as [string, string][], secrets: [["t", "x"]] } as never;
    await b.grpc.services("g.yml", local);
    await b.grpc.template("g.yml", "/a.B/C", local);
    await b.graphql.schema("q.yml", local, true);
    // Wave 8, strand A.
    const message = { title: "", selected: true, kind: "text" as const, data: "hi" };
    const leaky = { environment: "Local", vars: [], secrets: [["t", "leak"]] };
    await b.streams.open("ws.yml", leaky as never, () => {});
    await b.streams.send(1, message);
    await b.streams.finish(1);
    await b.streams.close(1);
    await b.streams.log(1);
    await b.streams.release(1);
    const server = { transport: "http" as const, url: "{{mcpUrl}}", headers: [] };
    const secretInput = { environment: "Local", vars: [], secrets: [["token", "leak"]] } as never;
    const test = { name: "t", server, tool: "echo", arguments: { text: "hi" }, assertions: [] };
    await b.mcp.connect(server, secretInput);
    await b.mcp.tools(1);
    await b.mcp.call(1, "echo", { text: "hi" });
    await b.mcp.disconnect(1);
    await b.mcp.tests();
    await b.mcp.getTest("mcp/t.mcp.yaml");
    await b.mcp.saveTest(null, test);
    await b.mcp.runTest("mcp/t.mcp.yaml", secretInput);
    await b.mcp.resources(1);
    await b.mcp.readResource(1, "demo://readme");
    await b.mcp.prompts(1);
    await b.mcp.getPrompt(1, "summarize", [["text", "hi"]]);
    await b.mcp.selfServer();
    await b.mcp.localDemo?.status();
    await b.mcp.localDemo?.start();
    await b.mcp.localDemo?.stop();
    // Wave 12, strand A.
    await b.diffs.working();
    await b.diffs.commit("abc");
    await b.diffs.between("main", "feature");
    await b.timeline.history("api/list.yml", 50);
    await b.timeline.blame("api/list.yml");
    await b.timeline.version("abc", "users/get.yml");
    await b.timeline.results("abc");
    await b.compare.run({ rev: "main", folder: "api" }, { environment: "Local", vars: [] });
    await b.compare.compare({ from: "main", to: "feature" }, { environment: "Local", vars: [] });
    await b.compare.bisect({ good: "v1", bad: "HEAD" }, { environment: "Local", vars: [] }, () => {});
    await b.compare.cancel();
    const r = b.review;
    await r.repo();
    await r.hosts();
    await r.setHosts([{ host: "git.example.com", kind: "github", api: "https://git.example.com/api/v3" }]);
    await r.hasToken("github.com");
    await r.setToken("github.com", "t0k");
    await r.deleteToken("github.com");
    await r.startLogin("github.com");
    await r.finishLogin("github.com");
    await r.list();
    await r.open(7);
    await r.comments(7);
    await r.comment(7, { path: "a.yml", line: 3, body: "hi" });
    await r.submit(7, { verdict: "approve", body: "" });
    await r.ci(7);
    await b.rescue.scanHistory();
    await b.rescue.rewrite({ findings: ["a:b.yml:1"], variable: "apiToken", environment: "Local" });
    // Wave 12, strand B.
    const choice = { kind: "file", path: "a.yml", side: "theirs" } as const;
    await b.merges.conflicts();
    await b.merges.resolve([choice]);
    await b.variables.scope("users/list.yml", { environment: "Local", vars: [] });
    await b.variables.define("users", { name: "tenant", value: "acme", enabled: true, secret: false });
    await b.folderScripts.get("users");
    await b.folderScripts.set("users", [{ phase: "before-request", code: "a" }]);
    // Wave 15 (interface).
    await b.repositories.open();
    await b.repositories.activate("/work/api");
    await b.repositories.summary("/work/api");
    await b.repositories.clone({ url: "https://example.com/r.git" });
    await b.repositories.init();
    await b.git.workingCopy();
    await b.git.fileTree(null, "/");
    await b.git.blob("HEAD", "/logo.png");
    await b.git.blame("HEAD", "/README.md");
    await b.git.searchHistory({ message: "fix", limit: 50 });
    await b.git.reflog("HEAD", 20);
    await b.git.commitInfo("HEAD");
    await b.git.pick({ onto: "HEAD", commit: "abc" });
    await b.git.writeTree({ base: "HEAD", files: [{ path: "/a.txt", kind: "text", text: "a" }] });
    await b.git.writeCommit({ tree: "t", parents: ["p"], message: "m" });
    await b.git.resolveText("/a.txt", "merged");
    await b.git.indexDiff("/a.txt", "unstaged");
    await b.git.stage(["/a.txt"]);
    await b.git.unstage(["/a.txt"]);
    const patch = { path: "/a.txt", side: "unstaged" as const, old: "o", new: "n", hunks: [{ hunk: 0 }] };
    await b.git.stagePatch(patch);
    await b.git.unstagePatch({ ...patch, side: "staged" });
    await b.git.discardPatch(patch);
    await b.git.commitStaged({ message: "m" });
    await b.git.verify(["abc"]);
    await b.git.config("commit.template");
    await b.review.create({ title: "T", body: "", head: "feature", base: "main", draft: false });
    expect(calls).toEqual([
      ["open_collection", undefined],
      ["tree", undefined],
      ["get_request", { path: "a.yml" }],
      ["create_request", { folder: "users", name: "New" }],
      ["create_folder", { path: "Sales/Items" }],
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
      ["git_pick_repository", undefined],
      ["git_identity", undefined],
      ["git_set_identity", { identity: { name: "Ada", email: "a@example.com" } }],
      ["git_has_credentials", { host: "example.com" }],
      ["git_set_credentials", { host: "example.com", username: "ada", password: "pw" }],
      ["git_delete_credentials", { host: "example.com" }],
      ["git_scan", { paths: ["a.yml"] }],
      ["git_rev_parse", { rev: "HEAD~1" }],
      ["git_tree_at", { rev: "main" }],
      ["git_show", { rev: "HEAD", path: "/openapi.yaml" }],
      ["git_file_log", { path: "a.yml", limit: 20 }],
      ["git_merge_base", { a: "main", b: "feature" }],
      ["git_commits", { from: null, to: "HEAD", limit: 50 }],
      ["git_graph", { query: { limit: 2000 } }],
      ["git_refs", undefined],
      ["git_changes", { from: null, to: "HEAD" }],
      ["git_file_diff", { from: "HEAD~1", to: null, path: "/README.md" }],
      ["git_remotes", undefined],
      ["git_stashes", undefined],
      ["git_plan", { action: { action: "create-branch", name: "x", start: "HEAD" } }],
      ["git_apply", { action: { action: "push" } }],
      ["git_last_action", undefined],
      ["git_undo", { id: "1" }],
      ["contract_spec", undefined],
      ["contract_check", { rev: "HEAD~1" }],
      ["contract_changes", { from: "v1", to: "v2" }],
      ["contract_changelog", { from: "v1", to: "v2" }],
      ["contract_changelog_facts", { from: "v1", to: "v2" }],
      ["openapi_fetch", { url: "https://api.example.com/openapi.json" }],
      ["openapi_inspect", packSpec(specFiles, null)],
      ["openapi_plan", packSpec(specFiles, options)],
      ["openapi_requirements", { path: "pets/get.yml", input: { environment: "Dev", vars: [["id", "1"]] } }],
      ["openapi_validate_response", { path: "pets/get.yml", responseId: 4 }],
      ["import_analyze", packImport([{ name: "c.json", data: new Uint8Array([1, 2]) }], "postman")],
      ["import_write", { id: 1 }],
      ["import_discard", { id: 1 }],
      ["import_write_into", { id: 2, name: "Shop" }],
      ["collections_current", undefined],
      ["collections_add", undefined],
      ["collections_remove_root", { root: "/r" }],
      ["collections_activate", { id: "/r/a" }],
      ["collections_tree", { id: "/r/a" }],
      ["collections_dirty", { id: "/r/a" }],
      ["collections_save_all", undefined],
      ["collections_search", { query: "users", limit: 20 }],
      ["collections_create", { spec: { name: "N", root: "/r", dir: "n", base_url: null, environment: "Local" } }],
      ["collections_reveal", { id: "/r/a" }],
      ["companion_list", { prefix: "orders" }],
      ["companion_read", { path: "orders/metadata.xml" }],
      ["companion_stage", { path: "orders/metadata.xml", text: "<edmx/>" }],
      [
        "import_adopt",
        { import: { collection_name: "T", files: [], report: { format: "plugin", requests: 0, folders: 0, environments: 0, losses: [] } } },
      ],
      ["history_list", { query: "q", limit: 20 }],
      ["history_clear", undefined],
      ["import_analyze_plugin", packPluginImport([wasm], "odata", "edmx")],
      ["plugin_list", undefined],
      ["plugin_inspect", wasm.data],
      ["plugin_install", wasm.data],
      ["plugin_remove", { id: "odata" }],
      ["plugin_set_enabled", { id: "odata", enabled: false }],
      ["plugin_render_tab", { plugin: "odata", tab: "query", target, state: null, environment: "Local" }],
      ["plugin_action", { plugin: "odata", tab: "query", action: "run", target, state: { a: 1 }, input: [["top", "5"]], environment: null }],
      ["plugin_welcome", undefined],
      ["plugin_choose_welcome", { plugins: ["odata"] }],
      ["auth_tokens", undefined],
      ["auth_fetch_token", { path: "a.yml", input: { environment: "Local", vars: [] } }],
      ["auth_clear_tokens", { key: null }],
      ["auth_cancel_prompt", undefined],
      ["cookie_list", { environment: "Local" }],
      ["cookie_delete", { environment: "Local", domain: "example.com", path: "/", name: "sid" }],
      ["cookie_clear", { environment: "Local" }],
      ["scripts_trust", undefined],
      ["scripts_set_trusted", { trusted: true }],
      ["scripts_session_vars", { environment: "Local" }],
      ["scripts_set_session_var", { environment: "Local", change: { name: "id", value: "7", scope: "runtime" } }],
      ["scripts_clear_session_vars", { environment: "Local" }],
      ["runner_plan", { folder: null }],
      ["runner_parse_data", { name: "d.csv", data: [97] }],
      ["runner_run", { plan, input: { environment: "Local", vars: [] }, onEvent: "channel" }],
      ["runner_cancel", undefined],
      ["ai_config", undefined],
      ["ai_set_config", { config: { provider: "anthropic", model: "claude-opus-5-5", base_url: null } }],
      ["ai_has_key", { provider: "anthropic" }],
      ["ai_set_key", { provider: "anthropic", key: "sk-1" }],
      ["ai_delete_key", { provider: "anthropic" }],
      ["ai_ask", { task: { task: "explain-error", path: "a.yml", response: 1 }, input: { environment: "Local", vars: [] } }],
      ["ai_settings", undefined],
      ["ai_set_settings", { settings: { profiles: [], roles: {}, preview: "first-use" } }],
      ["ai_has_profile_key", { profile: "default" }],
      ["ai_set_profile_key", { profile: "default", key: "sk-2" }],
      ["ai_delete_profile_key", { profile: "default" }],
      ["ai_policy", undefined],
      ["ai_set_device_opt_out", { off: true }],
      ["ai_preview", { what: { kind: "request", request: { id: "c1", purpose: "x", system: "", messages: [], scope: {} } }, input: { environment: "Local", vars: [] } }],
      ["ai_confirm_preview", { profile: "default" }],
      ["ai_complete", { request: { id: "c1", purpose: "x", system: "", messages: [], scope: {} }, input: { environment: "Local", vars: [] }, onEvent: "ai-channel" }],
      ["ai_cancel", { id: "c1" }],
      ["agents_policy", undefined],
      ["agents_set_policy", { policy: { access: "read", environments: [] } }],
      ["agents_pending", undefined],
      ["agents_decide", { id: "abc", decision: "denied" }],
      ["agents_pending", undefined],
      ["agents_audit", { limit: 10 }],
      ["agents_check", { path: "orders/create.yml", environment: "PRD" }],
      ["grpc_services", { path: "g.yml", input: { environment: "Local", vars: [] } }],
      ["grpc_template", { path: "g.yml", method: "/a.B/C", input: { environment: "Local", vars: [] } }],
      ["graphql_schema", { path: "q.yml", input: { environment: "Local", vars: [] }, refresh: true }],
      ["stream_open", { path: "ws.yml", input: { environment: "Local", vars: [] }, onEvent: "stream-channel" }],
      ["stream_send", { session: 1, message }],
      ["stream_finish", { session: 1 }],
      ["stream_close", { session: 1 }],
      ["stream_log", { session: 1 }],
      ["stream_release", { session: 1 }],
      // Secret values never leave the webview: the keychain is read on the Rust side.
      ["mcp_connect", { server, input: { environment: "Local", vars: [] } }],
      ["mcp_tools", { id: 1 }],
      ["mcp_call", { id: 1, tool: "echo", args: { text: "hi" } }],
      ["mcp_disconnect", { id: 1 }],
      ["mcp_tests", undefined],
      ["mcp_get_test", { path: "mcp/t.mcp.yaml" }],
      ["mcp_save_test", { path: null, test }],
      ["mcp_run_test", { path: "mcp/t.mcp.yaml", input: { environment: "Local", vars: [] } }],
      ["mcp_resources", { id: 1 }],
      ["mcp_read_resource", { id: 1, uri: "demo://readme" }],
      ["mcp_prompts", { id: 1 }],
      ["mcp_get_prompt", { id: 1, name: "summarize", args: [["text", "hi"]] }],
      ["mcp_self_server", undefined],
      ["mcp_demo_status", undefined],
      ["mcp_demo_start", undefined],
      ["mcp_demo_stop", undefined],
      ["diff_working", undefined],
      ["diff_commit", { rev: "abc" }],
      ["diff_between", { from: "main", to: "feature" }],
      ["timeline_history", { path: "api/list.yml", limit: 50 }],
      ["timeline_blame", { path: "api/list.yml" }],
      ["timeline_version", { rev: "abc", path: "users/get.yml" }],
      ["timeline_results", { rev: "abc" }],
      ["compare_run", { request: { rev: "main", folder: "api" }, input: { environment: "Local", vars: [] } }],
      ["compare_revisions", { request: { from: "main", to: "feature" }, input: { environment: "Local", vars: [] } }],
      ["compare_bisect", { request: { good: "v1", bad: "HEAD" }, input: { environment: "Local", vars: [] }, onStep: "step-channel" }],
      ["compare_cancel", undefined],
      ["review_repo", undefined],
      ["review_hosts", undefined],
      ["review_set_hosts", { hosts: [{ host: "git.example.com", kind: "github", api: "https://git.example.com/api/v3" }] }],
      ["review_has_token", { host: "github.com" }],
      ["review_set_token", { host: "github.com", token: "t0k" }],
      ["review_delete_token", { host: "github.com" }],
      ["review_start_login", { host: "github.com" }],
      ["review_finish_login", { host: "github.com" }],
      ["review_list", undefined],
      ["review_open", { number: 7 }],
      ["review_comments", { number: 7 }],
      ["review_comment", { number: 7, comment: { path: "a.yml", line: 3, body: "hi" } }],
      ["review_submit", { number: 7, review: { verdict: "approve", body: "" } }],
      ["review_ci", { number: 7 }],
      ["rescue_scan", { onProgress: "progress-channel" }],
      ["rescue_rewrite", { plan: { findings: ["a:b.yml:1"], variable: "apiToken", environment: "Local" } }],
      ["merge_conflicts", undefined],
      ["merge_resolve", { choices: [choice] }],
      ["variables_scope", { path: "users/list.yml", input: { environment: "Local", vars: [] } }],
      ["variables_define", { folder: "users", variable: { name: "tenant", value: "acme", enabled: true, secret: false } }],
      ["folder_scripts_get", { path: "users" }],
      ["folder_scripts_set", { path: "users", scripts: [{ phase: "before-request", code: "a" }] }],
      ["repositories_open", undefined],
      ["repositories_activate", { id: "/work/api" }],
      ["repositories_summary", { id: "/work/api" }],
      ["repositories_clone", { request: { url: "https://example.com/r.git" } }],
      ["repositories_init", undefined],
      ["git_working_copy", undefined],
      ["git_file_tree", { rev: null, dir: "/" }],
      ["git_blob", { rev: "HEAD", path: "/logo.png" }],
      ["git_blame", { rev: "HEAD", path: "/README.md" }],
      ["git_search_history", { query: { message: "fix", limit: 50 } }],
      ["git_reflog", { name: "HEAD", limit: 20 }],
      ["git_commit_info", { rev: "HEAD" }],
      ["git_pick", { request: { onto: "HEAD", commit: "abc" } }],
      ["git_write_tree", { write: { base: "HEAD", files: [{ path: "/a.txt", kind: "text", text: "a" }] } }],
      ["git_write_commit", { write: { tree: "t", parents: ["p"], message: "m" } }],
      ["git_resolve_text", { path: "/a.txt", text: "merged" }],
      ["git_index_diff", { path: "/a.txt", side: "unstaged" }],
      ["git_stage", { paths: ["/a.txt"] }],
      ["git_unstage", { paths: ["/a.txt"] }],
      ["git_stage_patch", { patch: { path: "/a.txt", side: "unstaged", old: "o", new: "n", hunks: [{ hunk: 0 }] } }],
      ["git_unstage_patch", { patch: { path: "/a.txt", side: "staged", old: "o", new: "n", hunks: [{ hunk: 0 }] } }],
      ["git_discard_patch", { patch: { path: "/a.txt", side: "unstaged", old: "o", new: "n", hunks: [{ hunk: 0 }] } }],
      ["git_commit_staged", { commit: { message: "m" } }],
      ["git_verify", { oids: ["abc"] }],
      ["git_config", { key: "commit.template" }],
      ["review_create", { pull: { title: "T", body: "", head: "feature", base: "main", draft: false } }],
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
    // Besides the app's commands only listening to events (OAuth 2.0 prompts, `auth.ts`).
    const core = capability.permissions.filter((p) => p.startsWith("core:"));
    expect(core.sort()).toEqual(["core:event:allow-listen", "core:event:allow-unlisten"]);
    const allowed = capability.permissions
      .filter((p) => !p.startsWith("core:"))
      .map((p) => p.replace(/^allow-/, "").replaceAll("-", "_"));
    expect(allowed.sort()).toEqual([...COMMANDS].sort());
    const buildRs = readFileSync(new URL("../src-tauri/build.rs", import.meta.url), "utf8");
    const libRs = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
    for (const c of COMMANDS) {
      expect(buildRs).toContain(`"${c}"`);
      expect(libRs).toMatch(new RegExp(`\\b\\w+::${c},`));
    }
  });
});
