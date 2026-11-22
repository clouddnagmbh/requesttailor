// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// External secrets through the companion (wave 5, strand E), against a fake `fetch`: what is
// sent (references only, with the token), caching, and the errors that tell what to do.

import { BackendError, type ExternalSecrets } from "@requesttailor/ui";
import { describe, expect, it } from "vitest";

import { ExternalSecretReader, NO_COMPANION_FOR_SECRETS } from "./external";

const companion = { url: "http://127.0.0.1:47812", token: "tok" };
const VALUE = ["s3cret", "-", "42"].join("");

const secrets: ExternalSecrets = {
  provider: "one-password",
  variables: [
    { name: "token", reference: "op://Dev/api/token", enabled: true },
    { name: "shared", reference: "op://Dev/api/shared", enabled: true },
    { name: "off", reference: "op://Dev/api/off", enabled: false },
  ],
};

interface Call {
  url: string;
  init: RequestInit;
}

function fakeFetch(answer: (body: { variables: { name: string }[] }) => [number, unknown]) {
  const calls: Call[] = [];
  const fetchFn = async (url: string, init: RequestInit) => {
    calls.push({ url, init });
    const [status, json] = answer(JSON.parse(init.body as string));
    return new Response(JSON.stringify(json), { status, headers: { "Content-Type": "application/json" } });
  };
  return { calls, fetchFn };
}

const echo = (body: { variables: { name: string }[] }): [number, unknown] => [
  200,
  { values: body.variables.map((v) => [v.name, `${v.name}-${VALUE}`]) },
];

async function rejection(p: Promise<unknown>): Promise<BackendError> {
  const e = await p.then(
    () => null,
    (err: unknown) => err,
  );
  expect(e).toBeInstanceOf(BackendError);
  return e as BackendError;
}

describe("ExternalSecretReader", () => {
  it("reads the enabled, missing entries once and caches them for the session", async () => {
    const { calls, fetchFn } = fakeFetch(echo);
    const reader = new ExternalSecretReader(async () => companion, fetchFn);
    expect(reader.cached(secrets)).toEqual([]);

    // `shared` has a value in the browser store: not read.
    expect(await reader.resolve(secrets, ["shared"])).toEqual([["token", `token-${VALUE}`]]);
    expect(calls).toHaveLength(1);
    expect(calls[0].url).toBe("http://127.0.0.1:47812/v1/secrets");
    expect(calls[0].init.method).toBe("POST");
    expect((calls[0].init.headers as Record<string, string>).Authorization).toBe("Bearer tok");
    expect(JSON.parse(calls[0].init.body as string)).toEqual({
      provider: "one-password",
      variables: [{ name: "token", reference: "op://Dev/api/token", enabled: true }],
    });

    // Cached: no second request; preview sees it.
    expect(await reader.resolve(secrets, ["shared"])).toEqual([["token", `token-${VALUE}`]]);
    expect(calls).toHaveLength(1);
    expect(reader.cached(secrets, ["shared"])).toEqual([["token", `token-${VALUE}`]]);

    // A changed reference is read again.
    const moved: ExternalSecrets = { ...secrets, variables: [{ name: "token", reference: "op://Prod/api/token", enabled: true }] };
    await reader.resolve(moved);
    expect(calls).toHaveLength(2);
    reader.clear();
    expect(reader.cached(secrets)).toEqual([]);
  });

  it("says how to start the companion when there is none", async () => {
    const reader = new ExternalSecretReader(async () => null, fakeFetch(echo).fetchFn);
    const e = await rejection(reader.resolve(secrets));
    expect(e.kind).toBe("unsupported");
    expect(e.message).toBe(`This environment reads secrets from 1Password. ${NO_COMPANION_FOR_SECRETS}`);
    expect(e.message).toContain("rt agent --allow-secrets");
    // Nothing to read: no companion needed.
    expect(await reader.resolve({ provider: "one-password", variables: [] })).toEqual([]);
  });

  it("passes on the companion's reasons", async () => {
    const off = new ExternalSecretReader(
      async () => companion,
      fakeFetch(() => [403, { error: "Restart it with `rt agent --allow-secrets`." }]).fetchFn,
    );
    const e403 = await rejection(off.resolve(secrets));
    expect(e403.kind).toBe("unsupported");
    expect(e403.message).toContain("--allow-secrets");

    const locked = new ExternalSecretReader(
      async () => companion,
      fakeFetch(() => [422, { error: "1Password: not signed in. Run `op signin`." }]).fetchFn,
    );
    const e422 = await rejection(locked.resolve(secrets));
    expect(e422.kind).toBe("shell");
    expect(e422.message).toBe("1Password: not signed in. Run `op signin`.");

    const unpaired = new ExternalSecretReader(async () => companion, fakeFetch(() => [401, "pairing required"]).fetchFn);
    expect((await rejection(unpaired.resolve(secrets))).message).toContain("no longer paired");

    const down = new ExternalSecretReader(async () => companion, async () => {
      throw new TypeError("Failed to fetch");
    });
    expect((await rejection(down.resolve(secrets))).message).toContain("did not answer");

    const short = new ExternalSecretReader(async () => companion, fakeFetch(() => [200, { values: [] }]).fetchFn);
    expect((await rejection(short.resolve(secrets))).message).toContain("no value for `token`");
  });
});
