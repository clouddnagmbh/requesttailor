// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Environment operations behind the manager dialog. Only the command layer (`Backend`) is used,
// so they work the same on desktop (OS keychain), web (encrypted browser storage) and mock.
// Rule for everything here: an `Environment` handed to `updateEnvironment` never carries a
// secret value; secret values only move through `backend.secrets`.

import type { Backend } from "../commands/backend";
import type { Environment, Variable } from "../commands/types";

/** Names that look like a production environment get a warning colour (visual only). */
export function isProductionName(name: string | null | undefined): boolean {
  return !!name && /prod|prd|\blive\b/i.test(name);
}

/** `base`, or `base 2`, `base 3`, ... — the first not in `taken`. */
export function uniqueName(base: string, taken: readonly string[]): string {
  if (!taken.includes(base)) return base;
  for (let n = 2; ; n++) if (!taken.includes(`${base} ${n}`)) return `${base} ${n}`;
}

/** What may be written to files: named variables, secrets without value. */
export function forFiles(env: Environment): Environment {
  return {
    ...env,
    variables: env.variables
      .filter((v) => v.name.trim() !== "")
      .map((v) => ({ ...v, value: v.secret ? null : v.value })),
  };
}

/** Problems that keep an environment from being stored, or `null`. */
export function validate(env: Environment, others: readonly string[]): string | null {
  const name = env.name.trim();
  if (!name) return "An environment needs a name.";
  if (others.includes(name)) return `An environment named "${name}" already exists.`;
  if (/[/\\]/.test(name)) return "Environment names cannot contain / or \\.";
  const seen = new Set<string>();
  for (const v of env.variables) {
    if (!v.name.trim()) continue;
    if (seen.has(v.name)) return `Variable "${v.name}" is defined twice.`;
    seen.add(v.name);
  }
  return null;
}

const secretNames = (env: Environment) => env.variables.filter((v) => v.secret && v.name).map((v) => v.name);

/** Copies the secret values of `env` to environment `to` (missing values are skipped). */
async function copySecrets(backend: Backend, env: Environment, to: string): Promise<void> {
  for (const name of secretNames(env)) {
    const value = await backend.secrets.get(env.name, name);
    if (value !== null) await backend.secrets.set(to, name, value);
  }
}

async function deleteSecrets(backend: Backend, env: Environment): Promise<void> {
  for (const name of secretNames(env)) await backend.secrets.delete(env.name, name);
}

export async function createEnvironment(backend: Backend, name: string): Promise<Environment> {
  const env: Environment = { name, variables: [], client_certificates: [], external_secrets: null };
  await backend.updateEnvironment(env);
  return env;
}

/** Stores `env` (in memory, unsaved). */
export async function storeEnvironment(backend: Backend, env: Environment): Promise<void> {
  await backend.updateEnvironment(forFiles(env));
}

/** Rename = create the new one, move secret values, delete the old one. Secrets are copied
 *  before anything is deleted, so a failing secret store loses nothing. */
export async function renameEnvironment(backend: Backend, env: Environment, to: string): Promise<Environment> {
  const renamed = { ...forFiles(env), name: to };
  await copySecrets(backend, env, to);
  await backend.updateEnvironment(renamed);
  await backend.deleteEnvironment(env.name);
  await deleteSecrets(backend, env);
  return renamed;
}

export async function duplicateEnvironment(backend: Backend, env: Environment, to: string): Promise<Environment> {
  const copy = { ...forFiles(env), name: to };
  await copySecrets(backend, env, to);
  await backend.updateEnvironment(copy);
  return copy;
}

export async function deleteEnvironment(backend: Backend, env: Environment): Promise<void> {
  await backend.deleteEnvironment(env.name);
  await deleteSecrets(backend, env);
}

/** Plain → secret: the value moves into the secret store and leaves the environment. */
export async function makeSecret(backend: Backend, env: Environment, index: number): Promise<Environment> {
  const v = env.variables[index];
  if (v.value !== null && v.value !== "") await backend.secrets.set(env.name, v.name, v.value);
  const next = withVariable(env, index, { ...v, value: null, secret: true });
  await storeEnvironment(backend, next);
  return next;
}

/** Secret → plain (after the user confirmed): the value goes into the environment, which will
 *  be written to a file on save, and leaves the secret store. */
export async function makePlain(backend: Backend, env: Environment, index: number): Promise<Environment> {
  const v = env.variables[index];
  const value = v.name ? await backend.secrets.get(env.name, v.name) : null;
  const next = withVariable(env, index, { ...v, value: value ?? "", secret: false });
  await storeEnvironment(backend, next);
  if (v.name) await backend.secrets.delete(env.name, v.name);
  return next;
}

/** A secret variable got a new name: its value moves along. */
export async function renameSecret(backend: Backend, env: string, from: string, to: string): Promise<void> {
  if (!from || from === to) return;
  const value = await backend.secrets.get(env, from);
  if (value !== null && to) await backend.secrets.set(env, to, value);
  await backend.secrets.delete(env, from);
}

export function withVariable(env: Environment, index: number, v: Variable): Environment {
  return { ...env, variables: env.variables.map((x, i) => (i === index ? v : x)) };
}
