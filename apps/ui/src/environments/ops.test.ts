// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { MockBackend } from "../commands/mock";
import type { Environment } from "../commands/types";
import {
  deleteEnvironment,
  duplicateEnvironment,
  forFiles,
  isProductionName,
  makePlain,
  makeSecret,
  renameEnvironment,
  renameSecret,
  storeEnvironment,
  uniqueName,
  validate,
} from "./ops";

async function backend() {
  const b = new MockBackend();
  await b.openCollection();
  return b;
}

const local = async (b: MockBackend) => (await b.environments()).find((e) => e.name === "Local")!;

describe("environment ops", () => {
  it("recognises production-looking names", () => {
    for (const n of ["PRD", "prod", "Production", "eu-prod-2", "Live"]) expect(isProductionName(n), n).toBe(true);
    for (const n of ["Local", "Staging", "QA", "delivery", "", null]) expect(isProductionName(n), String(n)).toBe(false);
  });

  it("finds unique names", () => {
    expect(uniqueName("Dev copy", ["Dev"])).toBe("Dev copy");
    expect(uniqueName("Dev copy", ["Dev copy", "Dev copy 2"])).toBe("Dev copy 3");
  });

  it("never hands secret values to files", () => {
    const env: Environment = {
      name: "X",
      client_certificates: [],
      external_secrets: null,
      variables: [
        { name: "token", value: "leak", enabled: true, secret: true },
        { name: "", value: "unnamed", enabled: true, secret: false },
        { name: "url", value: "http://x", enabled: false, secret: false },
      ],
    };
    expect(forFiles(env).variables).toEqual([
      { name: "token", value: null, enabled: true, secret: true },
      { name: "url", value: "http://x", enabled: false, secret: false },
    ]);
  });

  it("validates names", () => {
    const env = (name: string, vars: string[] = []): Environment => ({
      name,
      client_certificates: [],
      external_secrets: null,
      variables: vars.map((n) => ({ name: n, value: "", enabled: true, secret: false })),
    });
    expect(validate(env("New"), ["Local"])).toBeNull();
    expect(validate(env(" "), [])).toMatch(/name/);
    expect(validate(env("Local"), ["Local"])).toMatch(/already exists/);
    expect(validate(env("a/b"), [])).toMatch(/cannot contain/);
    expect(validate(env("A", ["x", "x"]), [])).toMatch(/twice/);
  });

  it("renames with secret values moved", async () => {
    const b = await backend();
    await b.secrets.set("Local", "token", "t");
    await renameEnvironment(b, await local(b), "Dev");
    expect((await b.environments()).map((e) => e.name)).toEqual(["PRD", "Dev"]);
    expect(await b.secrets.get("Dev", "token")).toBe("t");
    expect(await b.secrets.has("Local", "token")).toBe(false);
  });

  it("duplicates with secret values copied", async () => {
    const b = await backend();
    await b.secrets.set("Local", "token", "t");
    await duplicateEnvironment(b, await local(b), "Local copy");
    expect(await b.secrets.get("Local copy", "token")).toBe("t");
    expect(await b.secrets.get("Local", "token")).toBe("t");
  });

  it("deletes secret values with the environment", async () => {
    const b = await backend();
    await b.secrets.set("Local", "token", "t");
    await deleteEnvironment(b, await local(b));
    expect(await b.secrets.has("Local", "token")).toBe(false);
    expect((await b.environments()).map((e) => e.name)).toEqual(["PRD"]);
  });

  it("moves values between the environment and the secret store", async () => {
    const b = await backend();
    let env = await local(b);
    env = await makeSecret(b, env, 0);
    expect(await b.secrets.get("Local", "baseUrl")).toBe("http://127.0.0.1:47811");
    expect((await local(b)).variables[0]).toMatchObject({ value: null, secret: true });
    await makePlain(b, env, 0);
    expect(await b.secrets.has("Local", "baseUrl")).toBe(false);
    expect((await local(b)).variables[0]).toMatchObject({ value: "http://127.0.0.1:47811", secret: false });
  });

  it("moves a secret value when the variable is renamed", async () => {
    const b = await backend();
    await b.secrets.set("Local", "token", "t");
    await renameSecret(b, "Local", "token", "apiToken");
    expect(await b.secrets.get("Local", "apiToken")).toBe("t");
    expect(await b.secrets.has("Local", "token")).toBe(false);
  });

  it("stores and saves without secret values", async () => {
    const b = await backend();
    const env = await local(b);
    env.variables[1] = { ...env.variables[1], value: "should-not-be-written" };
    await storeEnvironment(b, env);
    await b.save();
    expect([...b.files.values()].join()).not.toContain("should-not-be-written");
  });
});
