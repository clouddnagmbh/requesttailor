// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The short forms of the "New…" dialog (wave 14, strand B): one per built-in kind, with their
// defaults, their checks and what they turn into (a request spec, a `NewCollection`, an
// environment, an MCP test). Pure, so every rule is tested without a DOM.

import type { CollectionEntry, Environment, McpTest, NewCollection, RequestSpec, TreeNode } from "../commands/types";
import { asKind, type NewRequestKind } from "../stream/kind";
import type { NewItemKind } from "./kinds";

export interface RequestForm {
  kind: NewRequestKind;
  name: string;
  collection: string | null;
  /** Folder path in that collection, `""` for its top level. */
  folder: string;
  /** HTTP only; the other kinds pick their own (`asKind`). */
  method: string;
  /** Optional; can be filled in the editor later. */
  url: string;
}

export interface FolderForm {
  kind: "folder";
  name: string;
  collection: string | null;
  /** The folder the new one goes into. */
  folder: string;
}

export interface EnvironmentForm {
  kind: "environment";
  name: string;
  collection: string | null;
  /** Name of an environment to copy the variables from (secret values never), or `null`. */
  copyFrom: string | null;
}

export interface CollectionForm {
  kind: "collection";
  name: string;
  /** `root`: a subfolder of `root`; `elsewhere`: the shell asks for a folder (a new root). */
  place: "root" | "elsewhere";
  root: string | null;
  /** Subfolder inside the root. Follows the name until the user edits it (`dirEdited`). */
  dir: string;
  dirEdited: boolean;
  /** Optional `baseUrl` variable. */
  baseUrl: string;
  /** Name of the first environment; `""` for none. */
  environment: string;
}

export interface McpTestForm {
  kind: "mcp-test";
  name: string;
  collection: string | null;
  transport: "http" | "stdio";
  url: string;
  /** The command line that starts a stdio server (desktop only). */
  command: string;
  /** Optional; picked in the MCP client after connecting. */
  tool: string;
}

export type NewForm = RequestForm | FolderForm | EnvironmentForm | CollectionForm | McpTestForm;

/** Field name → what is wrong with it. Empty: the form can be created. */
export type FormErrors = Partial<Record<string, string>>;

export const HTTP_METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] as const;

export const REQUEST_KINDS: readonly NewRequestKind[] = ["http", "graphql", "grpc", "websocket", "sse"];

export const isRequestKind = (k: NewItemKind): k is NewRequestKind => (REQUEST_KINDS as readonly string[]).includes(k);

/** The name a form starts with (selected, so typing replaces it). */
export const DEFAULT_NAMES: Record<NewItemKind, string> = {
  http: "New request",
  graphql: "New GraphQL query",
  grpc: "New gRPC call",
  websocket: "New WebSocket",
  sse: "New event stream",
  collection: "New collection",
  folder: "New folder",
  environment: "New environment",
  "mcp-test": "New MCP test",
};

/** The heading of each kind's form. */
export const FORM_TITLES: Record<NewItemKind, string> = {
  http: "New HTTP request",
  graphql: "New GraphQL request",
  grpc: "New gRPC request",
  websocket: "New WebSocket",
  sse: "New event stream",
  collection: "New collection",
  folder: "New folder",
  environment: "New environment",
  "mcp-test": "New MCP server test",
};

/** Example addresses shown in the empty URL field. */
export const URL_PLACEHOLDERS: Record<NewRequestKind, string> = {
  http: "https://api.example.com/users",
  graphql: "https://api.example.com/graphql",
  grpc: "grpc://localhost:50051",
  websocket: "wss://echo.example.com",
  sse: "https://api.example.com/events",
};

export const DEFAULT_ENVIRONMENT = "Local";

export interface FormContext {
  /** Where the dialog was opened for. */
  collection: string | null;
  folder: string;
  /** The collections of the sidebar (`AppState.collections`). */
  collections: readonly CollectionEntry[];
  /** The desktop app can start stdio MCP servers; the browser cannot. */
  desktop: boolean;
}

/** The form type of `kind`. */
export type FormOf<K extends NewItemKind> = K extends NewRequestKind ? RequestForm : Extract<NewForm, { kind: K }>;

/** The form of `kind`, filled with its defaults for `ctx`. */
export function initialForm<K extends NewItemKind>(kind: K, ctx: FormContext): FormOf<K> {
  return defaults(kind, ctx) as FormOf<K>;
}

function defaults(kind: NewItemKind, ctx: FormContext): NewForm {
  const name = DEFAULT_NAMES[kind];
  const collection = ctx.collection ?? ctx.collections[0]?.id ?? null;
  switch (kind) {
    case "folder":
      return { kind, name, collection, folder: ctx.folder };
    case "environment":
      return { kind, name, collection, copyFrom: null };
    case "mcp-test":
      return { kind, name, collection, transport: "http", url: "", command: "", tool: "" };
    case "collection": {
      const root = ctx.collections.find((c) => c.id === ctx.collection)?.root ?? ctx.collections[0]?.root ?? null;
      return {
        kind,
        name,
        place: root ? "root" : "elsewhere",
        root,
        dir: folderName(name),
        dirEdited: false,
        baseUrl: "",
        environment: DEFAULT_ENVIRONMENT,
      };
    }
    default:
      return { kind, name, collection, folder: ctx.folder, method: "GET", url: "" };
  }
}

/** A folder name for `name`: lower case, words joined with `-`, letters and digits of any
 *  script kept, everything else dropped (`"Payments API v2"` → `payments-api-v2`). */
export function folderName(name: string): string {
  return name
    .normalize("NFC")
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64)
    .replace(/-+$/, "");
}

/** `form` with `name` set; a collection's folder follows the name until it was edited. */
export function withName<F extends NewForm>(form: F, name: string): F {
  if (form.kind === "collection" && !form.dirEdited) return { ...form, name, dir: folderName(name) };
  return { ...form, name };
}

/** The folders of `tree` for a target picker: path, name and depth, top level first (`""`). */
export function folderOptions(tree: TreeNode | null | undefined): { path: string; name: string; depth: number }[] {
  const out = [{ path: "", name: "", depth: 0 }];
  const walk = (nodes: TreeNode[], depth: number) => {
    for (const n of nodes) {
      if (n.kind !== "folder") continue;
      out.push({ path: n.path, name: n.name, depth });
      walk(n.children, depth + 1);
    }
  };
  if (tree?.kind === "folder") walk(tree.children, 1);
  return out;
}

/** Display names and directory names of the folders directly in `folder` of `tree`. */
export function folderChildren(tree: TreeNode | null | undefined, folder: string): string[] {
  const find = (node: TreeNode | null | undefined): TreeNode | null => {
    if (!node || node.kind !== "folder") return null;
    if (node.path === folder || (folder === "" && node === tree)) return node;
    for (const c of node.children) {
      const hit = find(c);
      if (hit) return hit;
    }
    return null;
  };
  const at = folder === "" ? tree : find(tree);
  if (!at || at.kind !== "folder") return [];
  return at.children.flatMap((c) => (c.kind === "folder" ? [c.name, c.path.split("/").pop() ?? c.path] : []));
}

/** What the checks know about the target. */
export interface FormFacts {
  /** Environment names of the target collection (when known). */
  environments?: readonly string[];
  /** Folder names directly in the target folder (`folderChildren`). */
  siblings?: readonly string[];
  collections?: readonly CollectionEntry[];
}

const BAD_SEGMENT = /[\\/:*?"<>|\p{Cc}]/u;

function segmentProblem(name: string, what: string): string | null {
  const n = name.trim();
  if (n === "." || n === "..") return `A ${what} cannot be called “${n}”.`;
  if (BAD_SEGMENT.test(n)) return `A ${what} name cannot contain / \\ : * ? " < > |.`;
  return null;
}

const same = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();

/** What stops `form` from being created; empty when it can be. */
export function validate(form: NewForm, facts: FormFacts = {}): FormErrors {
  const errors: FormErrors = {};
  const name = form.name.trim();
  if (name === "") errors.name = "Give it a name.";
  if (form.kind !== "collection" && !form.collection) errors.collection = "Open or create a collection first.";
  switch (form.kind) {
    case "folder": {
      const bad = name && segmentProblem(name, "folder");
      if (bad) errors.name = bad;
      else if (name && facts.siblings?.some((s) => same(s, name))) errors.name = `There is already a folder “${name}” here.`;
      break;
    }
    case "environment": {
      const bad = name && segmentProblem(name, "environment");
      if (bad) errors.name = bad;
      else if (name && facts.environments?.some((e) => same(e, name))) errors.name = `There is already an environment “${name}”.`;
      break;
    }
    case "collection": {
      if (form.place === "root") {
        const dir = form.dir.trim();
        if (!form.root) errors.dir = "Choose where the collection goes.";
        else if (dir === "") errors.dir = "Give the folder a name.";
        else if (dir.startsWith("/") || dir.split("/").some((s) => s.trim() === "" || segmentProblem(s, "folder")))
          errors.dir = "Use a folder name, or names separated by /, without . or .. and without \\ : * ? \" < > |.";
        else if (facts.collections?.some((c) => c.root === form.root && same(c.dir, dir)))
          errors.dir = "Another collection already lives in this folder.";
      }
      const env = form.environment.trim();
      const bad = env && segmentProblem(env, "environment");
      if (bad) errors.environment = bad;
      break;
    }
    case "mcp-test":
      if (form.transport === "http" && form.url.trim() === "") errors.url = "Enter the server's address.";
      if (form.transport === "stdio" && form.command.trim() === "") errors.command = "Enter the command that starts the server.";
      break;
    default:
      break;
  }
  return errors;
}

/** `base` (a fresh request from `createRequest`) as the request `form` asks for. */
export function requestSpec(base: RequestSpec, form: RequestForm): RequestSpec {
  const spec = asKind(base, form.kind);
  const url = form.url.trim();
  return { ...spec, ...(form.kind === "http" ? { method: form.method } : {}), url: url === "" ? spec.url : url };
}

/** A new environment `name`, with the variables of `from` (secret variables keep their name
 *  and stay secret, never their value). */
export function newEnvironment(name: string, from: Environment | null): Environment {
  // A JSON copy: `from` may be reactive state, which `structuredClone` refuses.
  const copy = from ? (JSON.parse(JSON.stringify(from)) as Environment) : null;
  return {
    name: name.trim(),
    variables: (copy?.variables ?? []).map((v) => ({ ...v, value: v.secret ? null : v.value })),
    client_certificates: copy?.client_certificates ?? [],
    external_secrets: copy?.external_secrets ?? null,
  };
}

/** What `CollectionSetApi.create` gets for `form`. */
export function newCollection(form: CollectionForm): NewCollection {
  const baseUrl = form.baseUrl.trim();
  const environment = form.environment.trim();
  const here = form.place === "root" && form.root !== null;
  return {
    name: form.name.trim(),
    root: here ? form.root : null,
    dir: here ? form.dir.trim().replace(/\/+$/, "") : "",
    base_url: baseUrl === "" ? null : baseUrl,
    environment: environment === "" ? null : environment,
  };
}

/** A command line split into words; `"…"` and `'…'` keep spaces. */
export function splitCommand(line: string): string[] {
  const out: string[] = [];
  const re = /"([^"]*)"|'([^']*)'|(\S+)/g;
  for (const m of line.matchAll(re)) out.push(m[1] ?? m[2] ?? m[3]);
  return out;
}

/** The MCP test `form` stages (no arguments or assertions yet). */
export function mcpTest(form: McpTestForm): McpTest {
  const [command = "", ...args] = splitCommand(form.command);
  return {
    name: form.name.trim(),
    server:
      form.transport === "http"
        ? { transport: "http", url: form.url.trim(), headers: [] }
        : { transport: "stdio", command, args, env: [], cwd: null },
    tool: form.tool.trim(),
    arguments: {},
    assertions: [],
  };
}
