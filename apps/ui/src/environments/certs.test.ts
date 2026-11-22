// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { Environment } from "../commands/types";
import {
  addCertificate,
  certificateProblems,
  freshCertificate,
  isLiteralPassphrase,
  removeCertificate,
  setPath,
  setSourceKind,
  updateCertificate,
} from "./certs";

const env = (): Environment => ({
  name: "Local",
  variables: [],
  client_certificates: [],
  external_secrets: null,
});

describe("client certificate operations", () => {
  it("adds, updates and removes entries without touching the input", () => {
    const base = env();
    const one = addCertificate(base);
    expect(base.client_certificates).toHaveLength(0);
    expect(one.client_certificates).toEqual([freshCertificate()]);
    const two = addCertificate(updateCertificate(one, 0, { domain: "*.example.com", enabled: false }));
    expect(two.client_certificates[0]).toMatchObject({ domain: "*.example.com", enabled: false });
    expect(two.client_certificates[1].domain).toBe("");
    const left = removeCertificate(two, 0);
    expect(left.client_certificates).toEqual([freshCertificate()]);
    expect(updateCertificate(left, 5, { domain: "x" })).toBe(left);
  });

  it("switches between PEM and PKCS#12 keeping the path", () => {
    let e = setPath(addCertificate(env()), 0, "certificate_path", "certs/client.pem");
    e = setPath(e, 0, "private_key_path", "certs/client.key");
    const p12 = setSourceKind(e, 0, "pkcs12");
    expect(p12.client_certificates[0].source).toEqual({ type: "pkcs12", path: "certs/client.pem" });
    expect(setSourceKind(p12, 0, "pkcs12")).toBe(p12);
    const pem = setSourceKind(setPath(p12, 0, "path", "certs/client.p12"), 0, "pem");
    expect(pem.client_certificates[0].source).toEqual({
      type: "pem",
      certificate_path: "certs/client.p12",
      private_key_path: "",
    });
  });

  it("ignores path fields the source does not have", () => {
    const e = addCertificate(env());
    expect(setPath(e, 0, "path", "x.p12")).toBe(e);
    const p12 = setSourceKind(e, 0, "pkcs12");
    expect(setPath(p12, 0, "private_key_path", "k.pem")).toBe(p12);
    expect(setPath(e, 3, "certificate_path", "x")).toBe(e);
  });

  it("flags plain passphrases, missing fields and paths outside the collection", () => {
    expect(isLiteralPassphrase("")).toBe(false);
    expect(isLiteralPassphrase("{{certPassphrase}}")).toBe(false);
    expect(isLiteralPassphrase(" {{ pw }} ")).toBe(false);
    expect(isLiteralPassphrase("hunter2")).toBe(true);
    expect(isLiteralPassphrase("{{a}}{{b}}x")).toBe(true);

    const fresh = freshCertificate();
    expect(certificateProblems(fresh)).toHaveLength(2);
    const ok = {
      ...fresh,
      domain: "localhost:8443",
      source: { type: "pem" as const, certificate_path: "certs/c.pem", private_key_path: "certs/k.pem" },
      passphrase: "{{pw}}",
    };
    expect(certificateProblems(ok)).toEqual([]);
    expect(certificateProblems({ ...ok, source: { type: "pkcs12", path: "../c.p12" } })).toEqual([
      "Paths must stay inside the collection folder.",
    ]);
    expect(certificateProblems({ ...ok, source: { type: "pkcs12", path: "/etc/c.p12" } })).toHaveLength(1);
    expect(certificateProblems({ ...ok, passphrase: "secret" })[0]).toMatch(/secret variable/);
  });
});
