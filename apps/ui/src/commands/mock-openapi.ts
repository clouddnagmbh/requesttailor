// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The OpenAPI wizard and the run wizard in the mock backend, for the UI's tests. A small model
// of the core in TypeScript: JSON specs only (OpenAPI 3.x and Swagger 2.0, local `$ref`s), the
// overview, a plan with the wizard's choices that `ImportApi.writeInto` writes as real mock
// requests and environments, the requirements of a request against the kept spec, and the
// response check (with the same validation as the form, `openapi/form.ts`). `fetch` knows one
// URL (`https://mock.example/petstore.json`, the spec of the last `inspect` or a built-in
// petstore); any other URL fails with the web shell's CORS hint.

import { asSchema, effective, validateJson, validateText } from "../openapi/form";
import { BackendError, type ImportSource, type OpenApiApi, type SendInput } from "./backend";
import type {
  Environment,
  FolderImport,
  ImportPlan,
  InputField,
  Loss,
  OpenApiOptions,
  RequestSpec,
  ResponseCheck,
  SendRequirements,
  SpecOperation,
  SpecOverview,
} from "./types";

type Json = Record<string, unknown>;

export interface MockOpenApiHost {
  getRequest(path: string): Promise<RequestSpec>;
  createRequest(folder: string, name: string): Promise<string>;
  updateRequest(path: string, spec: RequestSpec): Promise<void>;
  environments(): Promise<Environment[]>;
  updateEnvironment(env: Environment): Promise<void>;
  hasSecret(env: string, name: string): Promise<boolean>;
  folderNames(): Promise<string[]>;
  /** Status and body text of a stored response. */
  response(id: number): { status: number; text: string | null; contentType: string } | null;
}

const METHODS = ["get", "put", "post", "delete", "options", "head", "patch", "trace"];
const decode = (d: Uint8Array) => new TextDecoder().decode(d);

export const MOCK_SPEC_URL = "https://mock.example/petstore.json";

function parse(files: ImportSource[]): { root: Json; main: string } {
  if (files.length === 0) throw new BackendError("import", "no file to import");
  const file = files.find((f) => /"(openapi|swagger)"\s*:/.test(decode(f.data))) ?? files[0]!;
  if (/\.zip$/i.test(file.name)) throw new BackendError("import", "the mock backend does not read zip archives");
  let root: unknown;
  try {
    root = JSON.parse(decode(file.data));
  } catch {
    throw new BackendError("import", `${file.name}: the mock backend reads JSON specs only`);
  }
  const r = asSchema(root);
  if (typeof r.openapi !== "string" && typeof r.swagger !== "string") {
    throw new BackendError("import", `not a valid OpenAPI file: ${file.name} has no \`openapi\` or \`swagger\` version`);
  }
  return { root: r, main: file.name };
}

function pointer(root: Json, ref: string): unknown {
  if (!ref.startsWith("#/")) return undefined;
  let cur: unknown = root;
  for (const part of ref.slice(2).split("/")) cur = asSchema(cur)[part.replace(/~1/g, "/").replace(/~0/g, "~")];
  return cur;
}

/** Inlines local `$ref`s (Swagger `#/definitions/X` too), bounded. */
function inline(root: Json, v: unknown, depth = 0): unknown {
  if (depth > 8 || v === null || typeof v !== "object") return v;
  if (Array.isArray(v)) return v.map((x) => inline(root, x, depth + 1));
  const o = v as Json;
  if (typeof o.$ref === "string") return inline(root, pointer(root, o.$ref) ?? {}, depth + 1);
  return Object.fromEntries(Object.entries(o).map(([k, x]) => [k, inline(root, x, depth + 1)]));
}

interface Server {
  url: string;
  description: string | null;
}

function servers(root: Json): Server[] {
  if (typeof root.swagger === "string") {
    if (typeof root.host !== "string") return [];
    const schemes = Array.isArray(root.schemes) && root.schemes.length ? (root.schemes as string[]) : ["https"];
    return schemes.map((s) => ({ url: `${s}://${root.host}${typeof root.basePath === "string" ? root.basePath : ""}`, description: null }));
  }
  return (Array.isArray(root.servers) ? root.servers : []).map((s) => ({
    url: String(asSchema(s).url ?? ""),
    description: typeof asSchema(s).description === "string" ? (asSchema(s).description as string) : null,
  }));
}

interface Op extends SpecOperation {
  raw: Json;
  params: Json[];
}

function operations(root: Json): Op[] {
  const out: Op[] = [];
  for (const [path, item] of Object.entries(asSchema(root.paths))) {
    const it = asSchema(inline(root, item, 0));
    const shared = Array.isArray(it.parameters) ? (it.parameters as Json[]) : [];
    for (const m of METHODS) {
      const op = asSchema(it[m]);
      if (!it[m]) continue;
      const own = Array.isArray(op.parameters) ? (op.parameters as Json[]) : [];
      const params = [...shared.filter((p) => !own.some((q) => q.name === p.name && q.in === p.in)), ...own];
      out.push({
        key: `${m.toUpperCase()} ${path}`,
        method: m.toUpperCase(),
        path,
        tags: Array.isArray(op.tags) ? (op.tags as string[]) : [],
        summary: typeof op.summary === "string" ? op.summary : null,
        operation_id: typeof op.operationId === "string" ? op.operationId : null,
        deprecated: op.deprecated === true,
        raw: op,
        params,
      });
    }
  }
  return out;
}

function security(root: Json): SpecOverview["security"] {
  const schemes = asSchema(asSchema(root.components).securitySchemes ?? root.securityDefinitions);
  return Object.entries(schemes).map(([name, raw]) => {
    const s = asSchema(raw);
    const type = String(s.type ?? "");
    const scheme = String(s.scheme ?? "").toLowerCase();
    if (type === "http" && scheme === "bearer") return { name, kind: "bearer", detail: String(s.bearerFormat ?? "token"), maps_to: "Bearer token {{token}}", variables: ["token"] };
    if ((type === "http" && scheme === "basic") || type === "basic") return { name, kind: "basic", detail: "user and password", maps_to: "Basic auth {{username}} / {{password}}", variables: ["username", "password"] };
    if (type === "apiKey") return { name, kind: "api-key", detail: `${s.in} ${s.name}`, maps_to: `API key ${s.name} in ${s.in} {{apiKey}}`, variables: ["apiKey"] };
    if (type === "oauth2") return { name, kind: "oauth2", detail: "OAuth 2.0", maps_to: "OAuth 2.0 (clientId, clientSecret)", variables: ["clientId", "clientSecret"] };
    return { name, kind: "other", detail: type, maps_to: null, variables: [] };
  });
}

function suggested(s: Server, i: number): string {
  const first = s.description?.split("\n")[0]?.trim();
  if (first) return first.slice(0, 60);
  const host = s.url.replace(/^[a-z]+:\/\//, "").split("/")[0]!;
  return host || `Server ${i + 1}`;
}

function overview(files: ImportSource[]): SpecOverview & { root: Json; ops: Op[] } {
  const { root, main } = parse(files);
  const info = asSchema(root.info);
  const ops = operations(root);
  const tags: SpecOverview["tags"] = [];
  for (const t of Array.isArray(root.tags) ? (root.tags as Json[]) : []) {
    tags.push({ name: String(t.name), description: typeof t.description === "string" ? t.description : null, operations: 0 });
  }
  for (const op of ops) {
    const tag = op.tags[0];
    if (!tag) continue;
    let t = tags.find((x) => x.name === tag);
    if (!t) tags.push((t = { name: tag, description: null, operations: 0 }));
    t.operations++;
  }
  const srv = servers(root);
  return {
    title: typeof info.title === "string" && info.title.trim() ? info.title : main.replace(/\.json$/i, ""),
    version: String(info.version ?? ""),
    description: typeof info.description === "string" ? info.description : null,
    spec_version: String(root.openapi ?? root.swagger),
    main_file: main,
    files: files.length,
    servers: srv.map((s, i) => ({
      url: s.url,
      description: s.description,
      suggested_name: suggested(s, i),
      variables: Object.entries(asSchema(asSchema((root.servers as Json[] | undefined)?.[i]).variables)).map(([name, v]) => ({
        name,
        default: String(asSchema(v).default ?? ""),
        values: Array.isArray(asSchema(v).enum) ? (asSchema(v).enum as string[]) : [],
        description: null,
      })),
    })),
    tags: tags.filter((t) => t.operations > 0),
    operations: ops.map(({ raw: _r, params: _p, ...op }) => op),
    security: security(root),
    losses: [],
    root,
    ops,
  };
}

const braces = (url: string) => url.replace(/\{([^}]+)\}/g, "{{$1}}");

interface PlannedRequest {
  folder: string;
  spec: RequestSpec;
}

interface Planned {
  plan: ImportPlan;
  requests: PlannedRequest[];
  environments: Environment[];
  spec: Json | null;
}

function emptySpec(name: string, method: string, url: string): RequestSpec {
  return {
    name,
    seq: null,
    method,
    url,
    headers: [],
    params: [],
    body: { type: "none" },
    auth: { type: "inherit" },
    vars: [],
    settings: { encode_url: null, follow_redirects: null, max_redirects: null, timeout_ms: null },
    scripts: [],
    assertions: [],
    actions: [],
  } as RequestSpec;
}

function exampleOf(root: Json, schema: unknown, prefer: OpenApiOptions["examples"]): string {
  const s = asSchema(inline(root, schema));
  if (prefer === "spec" && s.example !== undefined) return String(s.example);
  return "";
}

function buildRequest(root: Json, op: Op, o: OpenApiOptions): RequestSpec {
  const name = (o.naming === "operation-id" ? (op.operation_id ?? op.summary) : (op.summary ?? op.operation_id)) ?? `${op.method} ${op.path}`;
  const spec = emptySpec(name, op.method, `{{baseUrl}}${op.path.replace(/\{([^}]+)\}/g, ":$1")}`);
  for (const p of op.params) {
    if (p.in === "path") spec.params.push({ name: String(p.name), value: exampleOf(root, p.schema ?? p, o.examples), kind: "path", enabled: true });
    else if (p.in === "query") spec.params.push({ name: String(p.name), value: "", kind: "query", enabled: false });
  }
  const body = asSchema(op.raw.requestBody);
  const json = asSchema(asSchema(body.content)["application/json"]);
  const swaggerBody = op.params.find((p) => p.in === "body");
  if (json.schema || swaggerBody) {
    spec.headers.push({ name: "Content-Type", value: "application/json", enabled: true });
    spec.body = { type: "raw", kind: "json", data: "{}" };
  }
  return spec;
}

/** One mock backend's OpenAPI state. */
export function mockOpenApi(host: MockOpenApiHost): OpenApiApi & {
  /** For `ImportApi.writeInto` of a plan made here; `null` for other plans. */
  writeInto(id: number, name: string | null | undefined): Promise<FolderImport> | null;
  /** Drops a plan made here; `false` for other plans. */
  discardPlan(id: number): boolean;
} {
  const plans = new Map<number, Planned>();
  let nextId = 1000;
  let lastFetched: ImportSource[] | null = null;
  /** The spec kept in the mock collection (`keep_spec`), and where. */
  let kept: { root: Json; path: string } | null = null;

  const api = {
    fetch: async (url: string) => {
      if (url !== MOCK_SPEC_URL) {
        throw new BackendError(
          "import",
          `${new URL(url).host} does not let this browser read the spec (CORS). Pair the companion (rt agent) to fetch it, or download the file and drop it here.`,
        );
      }
      const files = lastFetched ?? [{ name: "petstore.json", data: new TextEncoder().encode(JSON.stringify(BUILTIN_PETSTORE)) }];
      return { url, files: files.map((f) => ({ name: f.name, text: decode(f.data) })), losses: [] };
    },

    inspect: async (files: ImportSource[]) => {
      const { root: _root, ops: _ops, ...o } = overview(files);
      lastFetched = files;
      return o;
    },

    plan: async (files: ImportSource[], options: OpenApiOptions) => {
      const o = overview(files);
      const chosen = o.ops.filter((op) => options.operations === null || options.operations.includes(op.key));
      const requests: PlannedRequest[] = chosen.map((op) => {
        const folder =
          options.grouping === "tag" ? (op.tags[0] ?? "") : options.grouping === "path" ? (op.path.split("/").find((s) => s && !s.startsWith("{")) ?? "") : "";
        return { folder, spec: buildRequest(o.root, op, options) };
      });
      const choices = options.servers ?? o.servers.map((s, index) => ({ index, environment: s.suggested_name }));
      const authVars = o.security.flatMap((s) => s.variables);
      const environments: Environment[] = choices
        .filter((c) => c.environment)
        .map((c) => ({
          name: c.environment!,
          variables: [
            { name: "baseUrl", value: braces(o.servers[c.index]?.url ?? ""), enabled: true, secret: false },
            ...authVars.map((v) => ({ name: v, value: null, enabled: true, secret: ["token", "password", "apiKey", "clientSecret"].includes(v) })),
          ],
          client_certificates: [],
          external_secrets: null,
        }));
      const losses: Loss[] = [];
      if (chosen.some((op) => op.raw.requestBody || op.params.some((p) => p.in === "body"))) {
        losses.push({ location: "collection", kind: "body", effect: "approximated", detail: "request bodies generated from the schema; edit the example values" });
      }
      losses.push({
        location: "collection",
        kind: "documentation",
        effect: "dropped",
        detail: "schemas and response definitions are API documentation and not imported; explicit response examples are kept as request examples",
      });
      const name = options.collection_name ?? o.title;
      const folders = new Set(requests.map((r) => r.folder).filter(Boolean)).size;
      const plan: ImportPlan = {
        id: nextId++,
        format: "openapi",
        collection_name: name,
        folder_name: name,
        files: requests.length + folders + environments.length + 1 + (options.keep_spec ? 1 : 0),
        report: { format: "openapi", requests: requests.length, folders, environments: environments.length, losses },
      };
      plans.set(plan.id, { plan, requests, environments, spec: options.keep_spec ? o.root : null });
      return plan;
    },

    requirements: async (path: string, input: SendInput): Promise<SendRequirements | null> => {
      if (!kept) return null;
      const request = await host.getRequest(path);
      const root = kept.root;
      const ops = operations(root);
      const route = request.url.replace(/^\{\{[^}]+\}\}/, "").replace(/^[a-z]+:\/\/[^/]+/i, "").split("?")[0]!;
      const segs = route.split("/").filter(Boolean);
      const op = ops.find((o) => {
        if (o.method !== request.method.toUpperCase()) return false;
        const t = o.path.split("/").filter(Boolean);
        return t.length === segs.length && t.every((s, i) => s.startsWith("{") || s === segs[i]);
      });
      if (!op) return null;
      const envs = await host.environments();
      const env = envs.find((e) => e.name === input.environment) ?? null;
      const fields: InputField[] = [];
      const field = (f: Partial<InputField> & Pick<InputField, "id" | "place" | "name">): InputField => ({
        required: false,
        secret: false,
        auth: false,
        value: null,
        problem: null,
        schema: null,
        description: null,
        example: null,
        ...f,
      });
      // Variables the URL uses that the environment does not define.
      for (const m of request.url.matchAll(/\{\{([^}]+)\}\}/g)) {
        const name = m[1]!;
        const v = env?.variables.find((x) => x.name === name && x.enabled);
        const set = v ? (v.secret ? await host.hasSecret(env!.name, name) : (v.value ?? "") !== "") : request.vars.some((x) => x.name === name && x.value);
        if (!set && !fields.some((f) => f.name === name)) {
          fields.push(
            field({
              id: `variable:${name}`,
              place: "variable",
              name,
              required: true,
              problem: { kind: "missing", message: `\`${name}\` has no value` },
              schema: name === "baseUrl" ? { type: "string", format: "uri" } : { type: "string" },
              example: name === "baseUrl" ? (servers(root).map((s) => braces(s.url))[0] ?? null) : null,
            }),
          );
        }
      }
      for (const p of op.params) {
        const place = p.in as InputField["place"];
        if (!["path", "query", "header"].includes(place)) continue;
        const name = String(p.name);
        const schema = inline(root, p.schema ?? { type: p.type, format: p.format, enum: p.enum });
        const value =
          place === "path"
            ? (request.params.find((x) => x.kind === "path" && x.name === name)?.value ?? "")
            : place === "query"
              ? (request.params.find((x) => x.kind === "query" && x.enabled && x.name === name)?.value ?? null)
              : (request.headers.find((h) => h.enabled && h.name.toLowerCase() === name.toLowerCase())?.value ?? null);
        const required = place === "path" || p.required === true;
        const text = value ?? "";
        const message = text === "" ? (required ? `\`${name}\` is required` : null) : validateText(schema, text, required);
        fields.push(
          field({
            id: `${place}:${name}`,
            place,
            name,
            required,
            value,
            schema,
            description: typeof p.description === "string" ? p.description : null,
            example: asSchema(schema).example !== undefined ? String(asSchema(schema).example) : null,
            problem: message ? { kind: text === "" ? "missing" : "invalid", message } : null,
          }),
        );
      }
      const rb = asSchema(op.raw.requestBody);
      const bodySchema = asSchema(asSchema(asSchema(rb.content)["application/json"]).schema);
      if (rb.content) {
        const schema = inline(root, bodySchema);
        const text = request.body.type === "raw" ? request.body.data : null;
        let problem: InputField["problem"] = null;
        if ((text ?? "").trim() === "") problem = rb.required === true ? { kind: "missing", message: "a request body is required" } : null;
        else {
          try {
            const errs = validateJson(schema, JSON.parse(text!));
            if (errs.length) problem = { kind: "invalid", message: "the body does not match the schema", details: errs.slice(0, 5).map((e) => `${e.path}: ${e.message}`) };
          } catch {
            problem = { kind: "invalid", message: "the body is not valid JSON" };
          }
        }
        fields.push(field({ id: "body", place: "body", name: "body", required: rb.required === true, value: text, schema, problem }));
      }
      fields.sort((a, b) => Number(!!b.problem) - Number(!!a.problem));
      return {
        operation: op.key,
        summary: op.summary,
        deprecated: op.deprecated,
        spec: kept.path,
        ready: fields.every((f) => !f.problem),
        fields,
        servers: servers(root).map((s) => ({ url: braces(s.url), description: s.description })),
      };
    },

    validateResponse: async (path: string, responseId: number): Promise<ResponseCheck | null> => {
      if (!kept) return null;
      const stored = host.response(responseId);
      if (!stored) return null;
      const request = await host.getRequest(path);
      const route = request.url.replace(/^\{\{[^}]+\}\}/, "").split("?")[0]!;
      const segs = route.split("/").filter(Boolean);
      const op = operations(kept.root).find((o) => {
        const t = o.path.split("/").filter(Boolean);
        return o.method === request.method.toUpperCase() && t.length === segs.length && t.every((s, i) => s.startsWith("{") || s === segs[i]);
      });
      if (!op) return null;
      const responses = asSchema(op.raw.responses);
      const key = String(stored.status) in responses ? String(stored.status) : `${String(stored.status)[0]}XX` in responses ? `${String(stored.status)[0]}XX` : "default" in responses ? "default" : null;
      const differences: ResponseCheck["differences"] = [];
      if (!key) {
        differences.push({ place: "status", path: "status", expected: `one of ${Object.keys(responses).join(", ")}`, got: String(stored.status) });
      } else {
        const entry = asSchema(inline(kept.root, responses[key]));
        const schema = asSchema(asSchema(asSchema(entry.content)["application/json"]).schema ?? entry.schema);
        if (Object.keys(schema).length && stored.text) {
          try {
            for (const e of validateJson(effective(schema), JSON.parse(stored.text))) {
              const [expected, got] = e.message.startsWith("required property") ? [`property ${/`(.+)`/.exec(e.message)?.[1]}`, "missing"] : [e.message.replace(/, found .*/, ""), e.message.replace(/.*found /, "")];
              differences.push({ place: "body", path: e.path, expected, got });
            }
          } catch {
            differences.push({ place: "body", path: "$", expected: "JSON", got: "not valid JSON" });
          }
        }
      }
      return { operation: op.key, status: stored.status, matched: key, differences, body_skipped: null };
    },

    discardPlan: (id: number) => plans.delete(id),

    writeInto: (id: number, wanted: string | null | undefined): Promise<FolderImport> | null => {
      const planned = plans.get(id);
      if (!planned) return null;
      return (async () => {
        plans.delete(id);
        const taken = new Set((await host.folderNames()).map((n) => n.toLowerCase()));
        const base = wanted?.trim() || planned.plan.folder_name;
        let name = base;
        for (let i = 2; taken.has(name.toLowerCase()); i++) name = `${base} ${i}`;
        for (const r of planned.requests) {
          const p = await host.createRequest(r.folder ? `${name}/${r.folder}` : name, r.spec.name);
          await host.updateRequest(p, r.spec);
        }
        const existing = await host.environments();
        const added: string[] = [];
        const extended: string[] = [];
        for (const env of planned.environments) {
          const have = existing.find((e) => e.name === env.name);
          if (!have) {
            await host.updateEnvironment(env);
            added.push(env.name);
          } else {
            const missing = env.variables.filter((v) => !have.variables.some((x) => x.name === v.name));
            if (missing.length) {
              await host.updateEnvironment({ ...have, variables: [...have.variables, ...missing] });
              extended.push(env.name);
            }
          }
        }
        let contract: string | null = null;
        if (planned.spec && !kept) {
          kept = { root: planned.spec, path: `${name}/openapi.json` };
          contract = kept.path;
        }
        return {
          folder: name,
          name,
          requests: planned.requests.length,
          folders: new Set(planned.requests.map((r) => r.folder).filter(Boolean)).size,
          environments_added: added,
          environments_extended: extended,
          files: contract ? [contract] : [],
          losses: [],
          contract_spec: contract,
        };
      })();
    },
  };
  return api;
}

/** A small petstore (OpenAPI 3.0), for `fetch` of `MOCK_SPEC_URL`. */
export const BUILTIN_PETSTORE = {
  openapi: "3.0.3",
  info: { title: "Swagger Petstore", version: "1.0.0", description: "A sample API that uses a petstore." },
  servers: [
    { url: "https://petstore.example.com/v1", description: "Production" },
    { url: "https://staging.petstore.example.com/v1", description: "Staging" },
  ],
  tags: [
    { name: "pets", description: "Everything about your pets" },
    { name: "store", description: "Orders" },
  ],
  paths: {
    "/pets": {
      get: { tags: ["pets"], summary: "List all pets", operationId: "listPets", parameters: [{ name: "limit", in: "query", schema: { type: "integer", minimum: 1, maximum: 100 } }], responses: { "200": { description: "ok" } } },
    },
    "/pets/{petId}": {
      get: {
        tags: ["pets"],
        summary: "Info for a specific pet",
        operationId: "showPetById",
        parameters: [{ name: "petId", in: "path", required: true, description: "The id of the pet to retrieve", schema: { type: "integer", minimum: 1, example: 42 } }],
        responses: { "200": { description: "A pet", content: { "application/json": { schema: { $ref: "#/components/schemas/Pet" } } } } },
      },
    },
    "/store/orders": {
      post: {
        tags: ["store"],
        summary: "Place an order",
        operationId: "placeOrder",
        requestBody: { required: true, content: { "application/json": { schema: { $ref: "#/components/schemas/Order" } } } },
        responses: { "201": { description: "created" } },
      },
    },
  },
  components: {
    schemas: {
      Pet: { type: "object", required: ["id", "name"], properties: { id: { type: "integer" }, name: { type: "string" }, tag: { type: "string" } } },
      Order: {
        type: "object",
        required: ["petId", "quantity"],
        properties: {
          petId: { type: "integer", minimum: 1 },
          quantity: { type: "integer", minimum: 1, maximum: 10 },
          status: { type: "string", enum: ["placed", "approved", "delivered"] },
          shipDate: { type: "string", format: "date" },
        },
      },
    },
  },
};
