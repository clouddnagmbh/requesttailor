// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The GraphQL schema of a request (wave 8, strand B): SDL from `Backend.graphql.schema`,
// parsed with graphql-js for completion and validation, and flattened for the schema
// browser (types, fields, docs, search).

import {
  buildSchema,
  isEnumType,
  isInputObjectType,
  isInterfaceType,
  isIntrospectionType,
  isObjectType,
  isScalarType,
  isSpecifiedScalarType,
  isUnionType,
  type GraphQLNamedType,
  type GraphQLSchema,
} from "graphql";

export function parseSchema(sdl: string): GraphQLSchema {
  return buildSchema(sdl);
}

export type TypeKind = "object" | "interface" | "union" | "enum" | "input" | "scalar";

export interface BrowserField {
  name: string;
  /** e.g. `(genre: Genre)`, empty without arguments. */
  args: string;
  /** e.g. `[Book!]!`; empty for enum values. */
  type: string;
  description: string | null;
  deprecated: string | null;
}

export interface BrowserType {
  name: string;
  kind: TypeKind;
  /** `query`, `mutation` or `subscription` for root types. */
  root: string | null;
  description: string | null;
  fields: BrowserField[];
}

function kindOf(t: GraphQLNamedType): TypeKind {
  if (isObjectType(t)) return "object";
  if (isInterfaceType(t)) return "interface";
  if (isUnionType(t)) return "union";
  if (isEnumType(t)) return "enum";
  if (isInputObjectType(t)) return "input";
  return "scalar";
}

function fieldsOf(t: GraphQLNamedType): BrowserField[] {
  if (isObjectType(t) || isInterfaceType(t)) {
    return Object.values(t.getFields()).map((f) => ({
      name: f.name,
      args: f.args.length
        ? `(${f.args.map((a) => `${a.name}: ${a.type.toString()}`).join(", ")})`
        : "",
      type: f.type.toString(),
      description: f.description ?? null,
      deprecated: f.deprecationReason ?? null,
    }));
  }
  if (isInputObjectType(t)) {
    return Object.values(t.getFields()).map((f) => ({
      name: f.name,
      args: "",
      type: f.type.toString(),
      description: f.description ?? null,
      deprecated: f.deprecationReason ?? null,
    }));
  }
  if (isEnumType(t)) {
    return t.getValues().map((v) => ({
      name: v.name,
      args: "",
      type: "",
      description: v.description ?? null,
      deprecated: v.deprecationReason ?? null,
    }));
  }
  if (isUnionType(t)) {
    return t.getTypes().map((m) => ({ name: m.name, args: "", type: "", description: null, deprecated: null }));
  }
  return [];
}

/** Types of `schema` for the browser: root types first, then by name; built-in scalars and
 *  introspection types left out. With `search`, only types whose name or description
 *  matches (all fields) or that have matching fields (only those), ignoring case. */
export function browse(schema: GraphQLSchema, search = ""): BrowserType[] {
  const roots = new Map<string, string>();
  for (const [op, t] of [
    ["query", schema.getQueryType()],
    ["mutation", schema.getMutationType()],
    ["subscription", schema.getSubscriptionType()],
  ] as const) {
    if (t) roots.set(t.name, op);
  }
  const order = (t: BrowserType) => (t.root ? ["query", "mutation", "subscription"].indexOf(t.root) : 3);
  const all: BrowserType[] = Object.values(schema.getTypeMap())
    .filter((t) => !isIntrospectionType(t) && !(isScalarType(t) && isSpecifiedScalarType(t)))
    .map((t) => ({
      name: t.name,
      kind: kindOf(t),
      root: roots.get(t.name) ?? null,
      description: t.description ?? null,
      fields: fieldsOf(t),
    }))
    .sort((a, b) => order(a) - order(b) || a.name.localeCompare(b.name));
  const q = search.trim().toLowerCase();
  if (!q) return all;
  const hit = (s: string | null) => (s ?? "").toLowerCase().includes(q);
  const out: BrowserType[] = [];
  for (const t of all) {
    if (hit(t.name) || hit(t.description)) {
      out.push(t);
      continue;
    }
    const fields = t.fields.filter((f) => hit(f.name) || hit(f.description) || hit(f.type) || hit(f.args));
    if (fields.length) out.push({ ...t, fields });
  }
  return out;
}

/** Why `text` is no valid variables object, or `null` (empty is fine). */
export function variablesProblem(text: string): string | null {
  if (text.trim() === "") return null;
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch (e) {
    return `Variables are not valid JSON: ${e instanceof Error ? e.message : String(e)}`;
  }
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? null
    : "Variables must be a JSON object.";
}
