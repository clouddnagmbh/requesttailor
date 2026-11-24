#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// A fake Git host for the pull-request review (wave 12, strand E). Tests run against this and
// never against github.com or gitlab.com. Node only, no dependencies; needs `git` on the PATH.
//
//   node fake-host.mjs [--github-port N] [--gitlab-port N] [--token T] [--dir D] [--max-per-page N] [--no-stdin-exit]
//
// - GitHub subset under /api/v3 (default port 48041), GitLab subset under /api/v4 (default
//   port 48042). Port 0 picks a free one. Both serve the same bare repository `acme/orders`
//   over Git's smart HTTP protocol (`git http-backend`) at `/acme/orders.git`; it holds
//   `refs/heads/main`, `refs/heads/feature/currency`, `refs/pull/1/head`, `refs/pull/2/head`,
//   `refs/merge-requests/1/head` and `refs/merge-requests/2/head`.
// - The only accepted token is --token (default `fake-token-1f4c9a`), sent as
//   `Authorization: Bearer <token>` or `PRIVATE-TOKEN`; anything else is 401.
// - Device code login: `POST /login/device/code` + `POST /login/oauth/access_token` (GitHub
//   port), `POST /oauth/authorize_device` + `POST /oauth/token` (GitLab port). The first poll
//   answers `authorization_pending`, the second hands out the token.
// - Test hooks: `GET /__state` (posted comments, reviews, approvals, `authorizations` seen),
//   `POST /__reset`. `created` in `/__state` holds the bodies of the pull requests opened with
//   `POST …/pulls` (GitHub) and `POST …/merge_requests` (GitLab); the head branch must exist.
// - On start it prints one line `READY {json}` (ports, work dir, repo, shas, token) to stdout.
//   It exits, removing its work dir, when stdin closes (unless --no-stdin-exit) or on SIGTERM.

import { execFileSync, spawn } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : fallback;
};
const GITHUB_PORT = Number(opt("github-port", 48041));
const GITLAB_PORT = Number(opt("gitlab-port", 48042));
const TOKEN = opt("token", "fake-token-1f4c9a");
// Caps `per_page`, to exercise paging with the two pull requests.
const MAX_PER = Number(opt("max-per-page", 100));
const work = opt("dir", null) ?? mkdtempSync(join(tmpdir(), "rt-fake-host-"));
const OWNER = "acme";
const NAME = "orders";
const PROJECT = `${OWNER}/${NAME}`;

// ---- the fixture repository ------------------------------------------------------------

const git = (cwd, ...a) =>
  execFileSync("git", a, {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "ignore"],
    env: {
      PATH: process.env.PATH,
      HOME: work,
      GIT_AUTHOR_NAME: "Ada Author",
      GIT_AUTHOR_EMAIL: "ada@example.test",
      GIT_COMMITTER_NAME: "Ada Author",
      GIT_COMMITTER_EMAIL: "ada@example.test",
      GIT_AUTHOR_DATE: "2026-11-01T10:00:00Z",
      GIT_COMMITTER_DATE: "2026-11-01T10:00:00Z",
    },
  }).trim();

mkdirSync(work, { recursive: true });
const bare = join(work, "repos", OWNER, `${NAME}.git`);
const tree = join(work, "tree");
mkdirSync(bare, { recursive: true });
mkdirSync(tree, { recursive: true });
git(bare, "init", "--bare", "-q", "-b", "main");
git(tree, "init", "-q", "-b", "main");
git(tree, "config", "commit.gpgsign", "false");
const fixture = join(here, "fixture");
const overlay = (dir) => cpSync(join(fixture, dir), tree, { recursive: true });
const commit = (message) => {
  git(tree, "add", "-A");
  git(tree, "commit", "-q", "-m", message);
  return git(tree, "rev-parse", "HEAD");
};
overlay("base");
const base = commit("Orders collection");
git(tree, "checkout", "-q", "-b", "feature/currency");
overlay("head");
const head1 = commit("Add currency, cancel an order, tune the list");
git(tree, "checkout", "-q", "main");
git(tree, "checkout", "-q", "-b", "chore/tenant");
overlay("draft");
const head2 = commit("Use the globex tenant locally");
git(tree, "push", "-q", bare, "main", "feature/currency", "chore/tenant");
for (const [ref, sha] of [
  ["refs/pull/1/head", head1],
  ["refs/pull/2/head", head2],
  ["refs/merge-requests/1/head", head1],
  ["refs/merge-requests/2/head", head2],
]) {
  git(bare, "update-ref", ref, sha);
}
const headFile = (sha, path) => git(bare, "show", `${sha}:${path}`);
const lineCount = (sha, path) => {
  try {
    return headFile(sha, path).split("\n").length;
  } catch {
    return -1;
  }
};

// ---- state -----------------------------------------------------------------------------

const PULLS = [
  { number: 1, title: "Add currency and cancel-order", author: "ada", draft: false, source: "feature/currency", target: "main", head: head1, updated: "2026-11-20T09:00:00Z", labels: [] },
  { number: 2, title: "Draft: use the globex tenant", author: "grace", draft: true, source: "chore/tenant", target: "main", head: head2, updated: "2026-11-19T09:00:00Z", labels: [] },
];
let state;
function reset() {
  state = { comments: [], reviews: [], approvals: [], notes: [], authorizations: [], created: [], devicePolls: 0, nextId: 100 };
  PULLS.length = 2;
  state.comments.push({ number: 1, id: 11, author: "grace", body: "Is `currency` required?", created: "2026-11-20T10:00:00Z", path: "orders/create-order.yml", line: 17, head: head1 });
  state.comments.push({ number: 1, id: 12, author: "grace", body: "Old remark on a line that moved.", created: "2026-11-20T10:30:00Z", path: "orders/create-order.yml", line: 300, head: "0".repeat(40) });
}
reset();

const pull = (n) => PULLS.find((p) => p.number === n);
/** The commit of branch `name` in the bare repository, or null. */
const branchSha = (name) => {
  try {
    return git(bare, "rev-parse", "--verify", "-q", `refs/heads/${name}`);
  } catch {
    return null;
  }
};
/** Opens a pull request (`create`); returns the new one or `{ error }` with the host's wording. */
function openPull({ title, head, base: target, draft }, author = "me") {
  const sha = branchSha(head);
  if (!sha) return { error: `head branch ${head} is not on the server` };
  if (!branchSha(target)) return { error: `base branch ${target} is not on the server` };
  if (PULLS.some((p) => p.source === head && p.target === target)) return { error: `A pull request already exists for ${PROJECT.split("/")[0]}:${head}.` };
  const number = Math.max(...PULLS.map((p) => p.number)) + 1;
  const made = { number, title, author, draft: Boolean(draft), source: head, target, head: sha, updated: "2026-11-24T09:00:00Z", labels: [] };
  PULLS.push(made);
  return { made };
}
const ci = {
  checks: [
    { name: "build", status: "completed", conclusion: "success", url: "https://ci.example.test/build/1" },
    { name: "rt run", status: "completed", conclusion: "failure", url: "https://ci.example.test/run/1" },
    { name: "lint", status: "in_progress", conclusion: null, url: "https://ci.example.test/lint/1" },
  ],
  statuses: [{ context: "legacy/ci", state: "success", target_url: "https://ci.example.test/legacy/1" }],
};

// ---- helpers ---------------------------------------------------------------------------

function send(res, status, body, headers = {}) {
  const json = typeof body === "string" || Buffer.isBuffer(body) ? body : JSON.stringify(body);
  res.writeHead(status, { "content-type": "application/json", ...headers });
  res.end(json);
}
const readBody = (req) =>
  new Promise((resolve) => {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => resolve(Buffer.concat(chunks)));
  });
const readJson = async (req) => {
  const b = await readBody(req);
  try {
    return b.length ? JSON.parse(b.toString("utf8")) : {};
  } catch {
    return {};
  }
};
function authorized(req) {
  const h = req.headers.authorization ?? "";
  const t = h.startsWith("Bearer ") ? h.slice(7) : (req.headers["private-token"] ?? "");
  return t === TOKEN;
}
const record = (req) => {
  const h = req.headers.authorization ?? req.headers["private-token"];
  if (h) state.authorizations.push(String(h));
};

/** A page of `items`, `per_page`/`page` as the real hosts do; `link` renders the next link. */
function paged(url, items) {
  const per = Math.min(Number(url.searchParams.get("per_page") ?? 30), MAX_PER);
  const page = Number(url.searchParams.get("page") ?? 1);
  const slice = items.slice((page - 1) * per, page * per);
  const next = page * per < items.length ? page + 1 : null;
  return { slice, next };
}

// ---- GitHub ----------------------------------------------------------------------------

const ghUser = (login) => ({ login });
const ghPull = (p, port) => ({
  number: p.number,
  title: p.title,
  user: ghUser(p.author),
  state: "open",
  draft: p.draft,
  merged_at: null,
  head: { ref: p.source, sha: p.head },
  base: { ref: p.target, sha: base },
  html_url: `http://127.0.0.1:${port}/${PROJECT}/pull/${p.number}`,
  updated_at: p.updated,
});
const ghComment = (c, head) => ({
  id: c.id,
  user: ghUser(c.author),
  body: c.body,
  created_at: c.created,
  path: c.path,
  line: c.head === head ? c.line : null,
  original_line: c.line,
  commit_id: c.head,
});

async function github(req, res, url, port) {
  const m = url.pathname.replace(/^\/api\/v3/, "");
  const repo = `/repos/${PROJECT}`;
  if (!authorized(req)) return send(res, 401, { message: "Bad credentials" });
  if (m === `${repo}/pulls` && req.method === "GET") {
    const open = PULLS.slice().sort((a, b) => (a.updated < b.updated ? 1 : -1));
    const { slice, next } = paged(url, open);
    const headers = next ? { link: `<http://127.0.0.1:${port}/api/v3${repo}/pulls?page=${next}&per_page=${url.searchParams.get("per_page") ?? 30}>; rel=\"next\"` } : {};
    return send(res, 200, slice.map((p) => ghPull(p, port)), headers);
  }
  if (m === `${repo}/pulls` && req.method === "POST") {
    const body = await readJson(req);
    state.created.push(body);
    if (!(body.title ?? "").trim()) return send(res, 422, { message: "Validation Failed", errors: [{ resource: "PullRequest", code: "missing_field", field: "title" }] });
    const r = openPull(body);
    if (r.error) return send(res, 422, { message: "Validation Failed", errors: [{ message: r.error }] });
    return send(res, 201, ghPull(r.made, port));
  }
  let r = m.match(new RegExp(`^${repo}/pulls/(\\d+)$`));
  if (r && req.method === "GET") {
    const p = pull(Number(r[1]));
    return p ? send(res, 200, ghPull(p, port)) : send(res, 404, { message: "Not Found" });
  }
  r = m.match(new RegExp(`^${repo}/pulls/(\\d+)/comments$`));
  if (r) {
    const p = pull(Number(r[1]));
    if (!p) return send(res, 404, { message: "Not Found" });
    if (req.method === "GET") {
      return send(res, 200, state.comments.filter((c) => c.number === p.number).map((c) => ghComment(c, p.head)));
    }
    const body = await readJson(req);
    const lines = lineCount(p.head, body.path);
    if (lines < 0 || !(body.line >= 1 && body.line <= lines) || body.commit_id !== p.head) {
      return send(res, 422, { message: "Validation Failed", errors: [{ resource: "PullRequestReviewComment", code: "invalid", field: "line" }] });
    }
    const c = { number: p.number, id: state.nextId++, author: "me", body: body.body, created: "2026-11-21T08:00:00Z", path: body.path, line: body.line, head: p.head };
    state.comments.push(c);
    return send(res, 201, ghComment(c, p.head));
  }
  r = m.match(new RegExp(`^${repo}/issues/(\\d+)/comments$`));
  if (r && req.method === "GET") {
    return send(res, 200, [{ id: 1, user: ghUser("grace"), body: "Please look at the contract notes.", created_at: "2026-11-20T09:30:00Z" }]);
  }
  r = m.match(new RegExp(`^${repo}/pulls/(\\d+)/reviews$`));
  if (r && req.method === "POST") {
    const p = pull(Number(r[1]));
    if (!p) return send(res, 404, { message: "Not Found" });
    const body = await readJson(req);
    if (body.event === "REQUEST_CHANGES" && !(body.body ?? "").trim()) {
      return send(res, 422, { message: "Validation Failed", errors: ["Review body can't be blank"] });
    }
    state.reviews.push({ number: p.number, event: body.event, body: body.body ?? "", commit_id: body.commit_id });
    return send(res, 200, { id: state.nextId++, state: body.event });
  }
  r = m.match(new RegExp(`^${repo}/commits/([0-9a-f]+)/check-runs$`));
  if (r) {
    return send(res, 200, { total_count: ci.checks.length, check_runs: ci.checks.map((c) => ({ name: c.name, status: c.status, conclusion: c.conclusion, html_url: c.url })) });
  }
  r = m.match(new RegExp(`^${repo}/commits/([0-9a-f]+)/status$`));
  if (r) return send(res, 200, { state: "success", statuses: ci.statuses });
  return send(res, 404, { message: "Not Found" });
}

// ---- GitLab ----------------------------------------------------------------------------

const MR_STATE = "opened";
const glMr = (p, port, full) => ({
  iid: p.number,
  title: p.title,
  author: { username: p.author },
  state: MR_STATE,
  draft: p.draft,
  source_branch: p.source,
  target_branch: p.target,
  sha: p.head,
  web_url: `http://127.0.0.1:${port}/${PROJECT}/-/merge_requests/${p.number}`,
  updated_at: p.updated,
  ...(full ? { diff_refs: { base_sha: base, head_sha: p.head, start_sha: base } } : {}),
});

async function gitlab(req, res, url, port) {
  const m = url.pathname.replace(/^\/api\/v4/, "");
  if (!authorized(req)) return send(res, 401, { message: "401 Unauthorized" });
  const proj = `/projects/${encodeURIComponent(PROJECT)}`;
  const projPlain = `/projects/${PROJECT}`;
  const p0 = m.startsWith(proj) ? proj : m.startsWith(projPlain) ? projPlain : null;
  if (!p0) return send(res, 404, { message: "404 Project Not Found" });
  const rest = m.slice(p0.length);
  if (rest === "/merge_requests" && req.method === "GET") {
    const open = PULLS.slice().sort((a, b) => (a.updated < b.updated ? 1 : -1));
    const { slice, next } = paged(url, open);
    return send(res, 200, slice.map((p) => glMr(p, port, false)), next ? { "x-next-page": String(next) } : { "x-next-page": "" });
  }
  if (rest === "/merge_requests" && req.method === "POST") {
    const body = await readJson(req);
    state.created.push(body);
    const draft = /^draft:/i.test(body.title ?? "");
    const r = openPull({ title: body.title ?? "", head: body.source_branch, base: body.target_branch, draft });
    if (r.error) return send(res, 409, { message: [r.error] });
    return send(res, 201, glMr(r.made, port, true));
  }
  let r = rest.match(/^\/merge_requests\/(\d+)$/);
  if (r && req.method === "GET") {
    const p = pull(Number(r[1]));
    return p ? send(res, 200, glMr(p, port, true)) : send(res, 404, { message: "404 Not found" });
  }
  r = rest.match(/^\/merge_requests\/(\d+)\/(discussions|notes|approve|unapprove|pipelines)$/);
  if (r) {
    const p = pull(Number(r[1]));
    if (!p) return send(res, 404, { message: "404 Not found" });
    const what = r[2];
    if (what === "discussions" && req.method === "GET") {
      const out = state.comments
        .filter((c) => c.number === p.number)
        .map((c) => ({ id: `d${c.id}`, notes: [{ id: c.id, author: { username: c.author }, body: c.body, created_at: c.created, system: false, position: { new_path: c.path, new_line: c.head === p.head ? c.line : null, old_line: null } }] }));
      out.push({ id: "dsys", notes: [{ id: 5, author: { username: "gitlab" }, body: "added 1 commit", created_at: "2026-11-20T08:00:00Z", system: true }] });
      out.push({ id: "dgen", notes: [{ id: 6, author: { username: "grace" }, body: "Please look at the contract notes.", created_at: "2026-11-20T09:30:00Z", system: false }] });
      return send(res, 200, out);
    }
    if (what === "discussions") {
      const body = await readJson(req);
      const pos = body.position ?? {};
      const lines = lineCount(p.head, pos.new_path);
      if (pos.head_sha !== p.head || pos.base_sha !== base || lines < 0 || !(pos.new_line >= 1 && pos.new_line <= lines)) {
        return send(res, 400, { message: "400 Bad request - Note {:line_code=>[\"can't be blank\"]}" });
      }
      const c = { number: p.number, id: state.nextId++, author: "me", body: body.body, created: "2026-11-21T08:00:00Z", path: pos.new_path, line: pos.new_line, head: p.head };
      state.comments.push(c);
      return send(res, 201, { id: `d${c.id}`, notes: [{ id: c.id, author: { username: "me" }, body: c.body, created_at: c.created, system: false, position: { new_path: c.path, new_line: c.line } }] });
    }
    if (what === "notes") {
      const body = await readJson(req);
      state.notes.push({ number: p.number, body: body.body });
      return send(res, 201, { id: state.nextId++, body: body.body });
    }
    if (what === "approve") {
      state.approvals.push({ number: p.number, approved: true });
      return send(res, 201, { approved: true });
    }
    if (what === "unapprove") {
      state.approvals.push({ number: p.number, approved: false });
      return send(res, 201, {});
    }
    if (what === "pipelines") return send(res, 200, [{ id: 900 + p.number, status: "failed", web_url: "https://ci.example.test/pipelines/900" }]);
  }
  r = rest.match(/^\/pipelines\/(\d+)\/jobs$/);
  if (r) {
    return send(res, 200, [
      { name: "build", status: "success", web_url: "https://ci.example.test/jobs/1", allow_failure: false },
      { name: "rt run", status: "failed", web_url: "https://ci.example.test/jobs/2", allow_failure: false },
      { name: "lint", status: "running", web_url: "https://ci.example.test/jobs/3", allow_failure: false },
      { name: "docs", status: "skipped", web_url: null, allow_failure: false },
    ]);
  }
  return send(res, 404, { message: "404 Not Found" });
}

// ---- device code -----------------------------------------------------------------------

async function deviceCode(req, res, url, kind, port) {
  const p = url.pathname;
  const form = async () => new URLSearchParams((await readBody(req)).toString("utf8"));
  const start = kind === "github" ? "/login/device/code" : "/oauth/authorize_device";
  const poll = kind === "github" ? "/login/oauth/access_token" : "/oauth/token";
  if (req.method === "POST" && p === start) {
    const f = await form();
    if (f.get("client_id") !== "fake-client-id") return send(res, 400, { error: "invalid_client" });
    state.devicePolls = 0;
    return send(res, 200, { device_code: "dev-code-1", user_code: "WDJB-MJHT", verification_uri: `http://127.0.0.1:${port}/login/device`, expires_in: 900, interval: 1 });
  }
  if (req.method === "POST" && p === poll) {
    const f = await form();
    if (f.get("device_code") !== "dev-code-1") return send(res, 400, { error: "invalid_grant" });
    if (state.devicePolls++ < 1) return send(res, 400, { error: "authorization_pending" });
    return send(res, 200, { access_token: TOKEN, token_type: "bearer", scope: "repo" });
  }
  return null;
}

// ---- Git over HTTP ---------------------------------------------------------------------

const backend = join(execFileSync("git", ["--exec-path"], { encoding: "utf8" }).trim(), "git-http-backend");
async function smartHttp(req, res, url) {
  const body = await readBody(req);
  const env = {
    PATH: process.env.PATH,
    GIT_PROJECT_ROOT: join(work, "repos"),
    GIT_HTTP_EXPORT_ALL: "1",
    PATH_INFO: url.pathname,
    REQUEST_METHOD: req.method,
    QUERY_STRING: url.search.replace(/^\?/, ""),
    CONTENT_TYPE: req.headers["content-type"] ?? "",
    CONTENT_LENGTH: String(body.length),
    REMOTE_USER: "fake",
    REMOTE_ADDR: "127.0.0.1",
  };
  if (req.headers["git-protocol"]) env.GIT_PROTOCOL = req.headers["git-protocol"];
  if (req.headers["content-encoding"]) env.HTTP_CONTENT_ENCODING = req.headers["content-encoding"];
  const child = spawn(backend, [], { env });
  const out = [];
  child.stdout.on("data", (c) => out.push(c));
  child.stdin.on("error", () => {});
  child.stdin.end(body);
  await new Promise((resolve) => child.on("close", resolve));
  const all = Buffer.concat(out);
  let sep = all.indexOf("\r\n\r\n");
  let skip = 4;
  if (sep < 0) {
    sep = all.indexOf("\n\n");
    skip = 2;
  }
  if (sep < 0) return send(res, 500, { message: "git http-backend failed" });
  const headers = {};
  let status = 200;
  for (const line of all.subarray(0, sep).toString("utf8").split(/\r?\n/)) {
    const i = line.indexOf(":");
    if (i < 0) continue;
    const k = line.slice(0, i).trim().toLowerCase();
    const v = line.slice(i + 1).trim();
    if (k === "status") status = Number.parseInt(v, 10);
    else headers[k] = v;
  }
  res.writeHead(status, headers);
  res.end(all.subarray(sep + skip));
}

// ---- servers ---------------------------------------------------------------------------

/** Like the real hosts' APIs, the fake answers browsers (CORS), so the web app can call it
 *  with `fetch` when no companion is paired. */
const CORS = {
  "access-control-allow-origin": "*",
  "access-control-allow-headers": "authorization,content-type,private-token,x-github-api-version,accept",
  "access-control-allow-methods": "GET,POST,PUT,DELETE,OPTIONS",
  "access-control-expose-headers": "link,x-next-page",
};

function handler(kind) {
  return async (req, res) => {
    for (const [k, v] of Object.entries(CORS)) res.setHeader(k, v);
    if (req.method === "OPTIONS") {
      res.writeHead(204);
      return res.end();
    }
    const port = req.socket.localPort;
    const url = new URL(req.url, `http://127.0.0.1:${port}`);
    try {
      if (url.pathname === "/__state") return send(res, 200, state);
      if (url.pathname === "/__reset" && req.method === "POST") {
        reset();
        return send(res, 200, {});
      }
      if (url.pathname.startsWith(`/${PROJECT}.git/`)) return await smartHttp(req, res, url);
      const device = await deviceCode(req, res, url, kind, port);
      if (device !== null) return;
      if (url.pathname.startsWith("/api/")) {
        record(req);
        if (kind === "github" && url.pathname.startsWith("/api/v3")) return await github(req, res, url, port);
        if (kind === "gitlab" && url.pathname.startsWith("/api/v4")) return await gitlab(req, res, url, port);
      }
      return send(res, 404, { message: "Not Found" });
    } catch (e) {
      return send(res, 500, { message: String(e?.stack ?? e) });
    }
  };
}

const listen = (kind, port) =>
  new Promise((resolve) => {
    const server = createServer(handler(kind));
    server.listen(port, "127.0.0.1", () => resolve(server));
  });
const gh = await listen("github", GITHUB_PORT);
const gl = await listen("gitlab", GITLAB_PORT);
const info = {
  github: gh.address().port,
  gitlab: gl.address().port,
  dir: work,
  repo: PROJECT,
  bare,
  token: TOKEN,
  clientId: "fake-client-id",
  base,
  head: head1,
  head2,
};
process.stdout.write(`READY ${JSON.stringify(info)}\n`);

let closing = false;
const stop = () => {
  if (closing) return;
  closing = true;
  gh.close();
  gl.close();
  try {
    rmSync(work, { recursive: true, force: true });
  } catch {}
  process.exit(0);
};
process.on("SIGTERM", stop);
process.on("SIGINT", stop);
if (!args.includes("--no-stdin-exit")) {
  process.stdin.on("end", stop);
  process.stdin.on("error", stop);
}
process.stdin.resume();
