// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// A local smart-HTTP Git server for the shell tests: Node's http server in front of
// `git http-backend` (CGI), serving the bare repositories in a temporary folder, optionally
// behind HTTP Basic auth. Also small helpers to run the `git` command line.

import { execFileSync, spawn } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync, mkdirSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

/** `git` without the user's configuration, with a fixed identity. */
export function gitCli(cwd: string, ...args: string[]): string {
  return execFileSync("git", args, {
    cwd,
    encoding: "utf8",
    env: {
      ...process.env,
      GIT_CONFIG_GLOBAL: "/dev/null",
      GIT_CONFIG_NOSYSTEM: "1",
      GIT_AUTHOR_NAME: "Colleague",
      GIT_AUTHOR_EMAIL: "colleague@example.com",
      GIT_COMMITTER_NAME: "Colleague",
      GIT_COMMITTER_EMAIL: "colleague@example.com",
    },
  });
}

export function hasGit(): boolean {
  try {
    execFileSync("git", ["--version"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

export interface GitServer {
  /** e.g. `http://127.0.0.1:47831` */
  url: string;
  /** Folder that holds the bare repositories. */
  root: string;
  /** Creates bare `<name>.git` with `files` committed on `main`; returns its URL. */
  repository(name: string, files: Record<string, string>): string;
  /** A working clone of `<name>.git` in a new temporary folder (for "a colleague"). */
  workingClone(name: string): string;
  stop(): Promise<void>;
}

export async function startGitServer(port: number, auth?: { username: string; password: string }): Promise<GitServer> {
  const root = mkdtempSync(join(tmpdir(), "rt-git-server-"));
  const temps: string[] = [root];
  const expected = auth ? `Basic ${Buffer.from(`${auth.username}:${auth.password}`).toString("base64")}` : null;

  const server: Server = createServer((req, res) => {
    const url = new URL(req.url ?? "/", "http://localhost");
    if (expected && req.headers.authorization !== expected) {
      res.writeHead(401, { "WWW-Authenticate": 'Basic realm="rt-test"' });
      res.end("authentication required");
      return;
    }
    const cgi = spawn("git", ["http-backend"], {
      env: {
        ...process.env,
        GIT_CONFIG_GLOBAL: "/dev/null",
        GIT_CONFIG_NOSYSTEM: "1",
        GIT_PROJECT_ROOT: root,
        GIT_HTTP_EXPORT_ALL: "1",
        PATH_INFO: decodeURIComponent(url.pathname),
        QUERY_STRING: url.search.replace(/^\?/, ""),
        REQUEST_METHOD: req.method ?? "GET",
        CONTENT_TYPE: req.headers["content-type"] ?? "",
        ...(req.headers["git-protocol"] ? { GIT_PROTOCOL: String(req.headers["git-protocol"]) } : {}),
        REMOTE_ADDR: "127.0.0.1",
        // Enables receive-pack (push) without extra configuration.
        REMOTE_USER: auth?.username ?? "anonymous",
      },
      stdio: ["pipe", "pipe", "pipe"],
    });
    req.pipe(cgi.stdin);
    let head = Buffer.alloc(0);
    let headDone = false;
    cgi.stdout.on("data", (chunk: Buffer) => {
      if (headDone) {
        res.write(chunk);
        return;
      }
      head = Buffer.concat([head, chunk]);
      const end = head.indexOf("\r\n\r\n");
      const endLf = head.indexOf("\n\n");
      const [at, sep] = end >= 0 && (endLf < 0 || end < endLf) ? [end, 4] : [endLf, 2];
      if (at < 0) return;
      headDone = true;
      let status = 200;
      const headers: Record<string, string> = {};
      for (const line of head.subarray(0, at).toString("latin1").split(/\r?\n/)) {
        const i = line.indexOf(":");
        if (i < 0) continue;
        const name = line.slice(0, i).trim();
        const value = line.slice(i + 1).trim();
        if (name.toLowerCase() === "status") status = parseInt(value, 10);
        else headers[name] = value;
      }
      res.writeHead(status, headers);
      res.write(head.subarray(at + sep));
    });
    cgi.stderr.on("data", (d) => process.stderr.write(`git http-backend: ${d}`));
    cgi.on("close", () => {
      if (!headDone) res.writeHead(500).end("git http-backend failed");
      else res.end();
    });
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", () => resolve());
  });
  const base = `http://127.0.0.1:${port}`;

  return {
    url: base,
    root,
    repository(name, files) {
      const work = mkdtempSync(join(tmpdir(), "rt-git-seed-"));
      temps.push(work);
      gitCli(work, "init", "-q", "-b", "main");
      for (const [p, text] of Object.entries(files)) {
        mkdirSync(dirname(join(work, p)), { recursive: true });
        writeFileSync(join(work, p), text);
      }
      gitCli(work, "add", "-A");
      gitCli(work, "commit", "-q", "-m", "Seed collection");
      gitCli(root, "clone", "-q", "--bare", work, `${name}.git`);
      gitCli(join(root, `${name}.git`), "symbolic-ref", "HEAD", "refs/heads/main");
      return `${base}/${name}.git`;
    },
    workingClone(name) {
      const work = mkdtempSync(join(tmpdir(), "rt-git-colleague-"));
      temps.push(work);
      gitCli(work, "clone", "-q", join(root, `${name}.git`), ".");
      return work;
    },
    async stop() {
      await new Promise<void>((resolve) => server.close(() => resolve()));
      for (const t of temps) rmSync(t, { recursive: true, force: true });
    },
  };
}

/** Starts a process and resolves stdout lines that match, in order (also lines already
 *  printed and not taken yet). */
export function startProcess(bin: string, args: string[]) {
  const child = spawn(bin, args, { stdio: ["ignore", "pipe", "pipe"] });
  const waiting: { re: RegExp; resolve: (m: RegExpMatchArray) => void; reject: (e: Error) => void }[] = [];
  const seen: string[] = [];
  let stderr = "";
  let buffer = "";
  child.stderr!.on("data", (d) => (stderr += String(d)));
  child.stdout!.on("data", (d) => {
    buffer += String(d);
    const lines = buffer.split("\n");
    buffer = lines.pop() ?? "";
    for (const line of lines) {
      const i = waiting.findIndex((w) => w.re.test(line));
      if (i < 0) {
        seen.push(line);
        continue;
      }
      const [w] = waiting.splice(i, 1);
      w.resolve(line.match(w.re)!);
    }
  });
  child.once("exit", (code) => {
    for (const w of waiting.splice(0)) w.reject(new Error(`${bin} exited (${code}): ${stderr}`));
  });
  return {
    child,
    stderr: () => stderr,
    line(re: RegExp): Promise<RegExpMatchArray> {
      const i = seen.findIndex((l) => re.test(l));
      if (i >= 0) return Promise.resolve(seen.splice(i, 1)[0].match(re)!);
      return new Promise((resolve, reject) => waiting.push({ re, resolve, reject }));
    },
  };
}
