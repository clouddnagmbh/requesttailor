// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Pure operations on the TLS client certificates of an environment (wave 5, strand D). Each
// returns a new environment; `ClientCertificates.svelte` hands it to `onchange`.

import type { CertificateSource, ClientCertificate, Environment } from "../commands/types";

export type SourceKind = CertificateSource["type"];

/** A new, enabled PEM entry with empty fields. */
export function freshCertificate(): ClientCertificate {
  return {
    domain: "",
    source: { type: "pem", certificate_path: "", private_key_path: "" },
    passphrase: "",
    enabled: true,
  };
}

function withCerts(env: Environment, certs: ClientCertificate[]): Environment {
  return { ...env, client_certificates: certs };
}

export function addCertificate(env: Environment): Environment {
  return withCerts(env, [...env.client_certificates, freshCertificate()]);
}

export function removeCertificate(env: Environment, index: number): Environment {
  return withCerts(
    env,
    env.client_certificates.filter((_, i) => i !== index),
  );
}

/** Replaces fields of entry `index`; out-of-range indexes leave `env` unchanged. */
export function updateCertificate(
  env: Environment,
  index: number,
  patch: Partial<Omit<ClientCertificate, "source">>,
): Environment {
  if (index < 0 || index >= env.client_certificates.length) return env;
  return withCerts(
    env,
    env.client_certificates.map((c, i) => (i === index ? { ...c, ...patch } : c)),
  );
}

/** Switches entry `index` between PEM and PKCS#12. A path carries over: the PEM certificate
 *  path becomes the archive path and back, so a mistaken switch loses nothing visible. */
export function setSourceKind(env: Environment, index: number, kind: SourceKind): Environment {
  const cert = env.client_certificates[index];
  if (!cert || cert.source.type === kind) return env;
  const source: CertificateSource =
    cert.source.type === "pem"
      ? { type: "pkcs12", path: cert.source.certificate_path }
      : { type: "pem", certificate_path: cert.source.path, private_key_path: "" };
  return withCerts(
    env,
    env.client_certificates.map((c, i) => (i === index ? { ...c, source } : c)),
  );
}

export type PathField = "certificate_path" | "private_key_path" | "path";

/** Sets one path of entry `index`; a field the entry's source does not have is ignored. */
export function setPath(env: Environment, index: number, field: PathField, value: string): Environment {
  const cert = env.client_certificates[index];
  if (!cert) return env;
  let source: CertificateSource;
  if (cert.source.type === "pem" && field !== "path") source = { ...cert.source, [field]: value };
  else if (cert.source.type === "pkcs12" && field === "path") source = { ...cert.source, path: value };
  else return env;
  return withCerts(
    env,
    env.client_certificates.map((c, i) => (i === index ? { ...c, source } : c)),
  );
}

/** A passphrase typed as plain text ends up in the environment file. */
export function isLiteralPassphrase(passphrase: string): boolean {
  const p = passphrase.trim();
  return p !== "" && !/^\{\{[^}]+\}\}$/.test(p);
}

/** Problems shown next to an entry (empty: fine). */
export function certificateProblems(cert: ClientCertificate): string[] {
  const problems: string[] = [];
  if (!cert.domain.trim()) problems.push("Enter the host this certificate is for.");
  const paths = cert.source.type === "pem" ? [cert.source.certificate_path, cert.source.private_key_path] : [cert.source.path];
  if (paths.some((p) => !p.trim())) problems.push("Enter the file path(s), relative to the collection folder.");
  if (paths.some((p) => p.startsWith("/") || /^[A-Za-z]:[\\/]/.test(p) || p.split(/[\\/]/).includes(".."))) {
    problems.push("Paths must stay inside the collection folder.");
  }
  if (isLiteralPassphrase(cert.passphrase)) {
    problems.push("The passphrase is stored in the file as typed; use a secret variable such as {{certPassphrase}}.");
  }
  return problems;
}
