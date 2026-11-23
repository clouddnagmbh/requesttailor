// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// "Fill and send" of the run wizard: the values of the form → the request as edited (path,
// query and header parameters, the body, request variables) and the writes to an environment
// (plain variables in the environment file, secrets only in the keychain / secret store, the
// file gets the variable with `secret: true` and no value). Nothing is saved; like any edit.
// Pure, unit-tested in apply.test.ts.

import type { Environment, InputField, RequestSpec, SendRequirements, Variable } from "../commands/types";
import { paramsFromUrl, urlFromParams } from "../editor/url";

/** Where a variable's value goes. */
export type VarLayer = "environment" | "request";

export interface FormState {
  /** By `InputField.id`; the body is JSON text. */
  values: Record<string, string>;
  /** Variable fields only. */
  layers: Record<string, VarLayer>;
  /** The environment variables and secrets go to; a name not in the collection is created. */
  environment: string | null;
}

export interface ApplyPlan {
  spec: RequestSpec;
  /** The request changed. */
  changed: boolean;
  /** The environment to create or update, or `null` if it does not change. */
  environment: Environment | null;
  /** Values for the secret store of `environment`. */
  secrets: { environment: string; name: string; value: string }[];
  /** Why "Fill and send" cannot go ahead (a secret but no environment name), or `null`. */
  blocked: string | null;
}

/** Default layer of a variable field: the environment for `baseUrl`, server variables and
 *  auth (they differ per stage), the request for anything else. */
export function defaultLayer(field: InputField, servers: SendRequirements["servers"]): VarLayer {
  if (field.secret || field.auth || field.name === "baseUrl") return "environment";
  if (servers.some((s) => s.url.includes(`{{${field.name}}}`))) return "environment";
  return "request";
}

/** Initial form values: what the request has; empty where nothing is set. */
export function initialState(req: SendRequirements, environment: string | null): FormState {
  const values: Record<string, string> = {};
  const layers: Record<string, VarLayer> = {};
  for (const f of req.fields) {
    values[f.id] = f.secret ? "" : (f.value ?? "");
    if (f.place === "variable") layers[f.id] = defaultLayer(f, req.servers);
  }
  return { values, layers, environment };
}

const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;

function setVar(list: Variable[], name: string, value: string | null, secret: boolean): Variable[] {
  const out = list.map((v) => ({ ...v }));
  const at = out.findIndex((v) => v.name === name);
  const next: Variable = { name, value: secret ? null : value, enabled: true, secret };
  if (at >= 0) out[at] = { ...out[at]!, ...next };
  else out.push(next);
  return out;
}

/** What "Fill and send" does with `state`. Only fields whose value differs from the request
 *  (or that had a problem) are written. */
export function planApply(spec: RequestSpec, req: SendRequirements, state: FormState, environments: Environment[]): ApplyPlan {
  const out = clone(spec);
  let changed = false;
  let env: Environment | null = null;
  const secrets: ApplyPlan["secrets"] = [];
  let blocked: string | null = null;
  const envName = state.environment?.trim() || null;
  const envFor = (): Environment | null => {
    if (!envName) return null;
    if (!env) {
      const found = environments.find((e) => e.name === envName);
      env = found ? clone(found) : { name: envName, variables: [], client_certificates: [], external_secrets: null };
    }
    return env;
  };

  for (const f of req.fields) {
    const value = state.values[f.id] ?? "";
    const original = f.secret ? "" : (f.value ?? "");
    if (value === original && !f.problem) continue;
    if (value === "" && f.place !== "body" && !f.problem) continue;
    switch (f.place) {
      case "variable": {
        if (value === "") break;
        const layer = f.secret ? "environment" : (state.layers[f.id] ?? "environment");
        if (layer === "environment") {
          const e = envFor();
          if (!e) {
            blocked = `Choose an environment for ${f.name}.`;
            break;
          }
          e.variables = setVar(e.variables, f.name, value, f.secret);
          if (f.secret) secrets.push({ environment: e.name, name: f.name, value });
        } else {
          out.vars = setVar(out.vars, f.name, value, false);
          changed = true;
        }
        break;
      }
      case "path": {
        const params = paramsFromUrl(out.url, out.params);
        const row = params.find((p) => p.kind === "path" && p.name === f.name);
        if (row) row.value = value;
        else params.push({ name: f.name, value, kind: "path", enabled: true });
        out.params = params;
        changed = true;
        break;
      }
      case "query": {
        const row = out.params.find((p) => p.kind === "query" && p.name === f.name);
        if (row) {
          row.value = value;
          row.enabled = value !== "" || row.enabled;
        } else if (value !== "") out.params.push({ name: f.name, value, kind: "query", enabled: true });
        out.url = urlFromParams(out.url, out.params);
        out.params = paramsFromUrl(out.url, out.params);
        changed = true;
        break;
      }
      case "header": {
        const row = out.headers.find((h) => h.name.toLowerCase() === f.name.toLowerCase());
        if (row) {
          row.value = value;
          row.enabled = true;
        } else out.headers.push({ name: f.name, value, enabled: true });
        changed = true;
        break;
      }
      case "body": {
        if (value.trim() === "") break;
        out.body = { type: "raw", kind: "json", data: value };
        if (!out.headers.some((h) => h.enabled && h.name.toLowerCase() === "content-type")) {
          out.headers.push({ name: "Content-Type", value: "application/json", enabled: true });
        }
        changed = true;
        break;
      }
    }
  }
  return { spec: out, changed, environment: env, secrets, blocked };
}

/** Problems the form still shows (as the user typed), by field id. */
export type FieldErrors = Record<string, string>;
