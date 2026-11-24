// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ContractApi` of the mock backend (wave 12, strand F), for the contract UI's tests and for UI
// development. The mock has no OpenAPI file in its Git (its trees hold request specs), so
// there is no spec until `enableDemo()` (tests call it through `window.__rt.backend.contract`;
// `?backend=mock&contract=1` does it at start): then the collection is "checked" against a
// scripted Orders API 2.0.0 and `changes` returns scripted breaking and other changes. The
// answers do not depend on the requests; the real core (rt-format) is tested in Rust.

import { BackendError, type ContractApi } from "./backend";
import type { MockWave12Host } from "./mock-wave12";
import type { ChangelogFacts, ContractReport, ContractSpec, SpecChange, SpecChanges } from "./types";

const SPEC: ContractSpec = { path: "openapi.yaml", title: "Orders API", version: "2.0.0", configured: false };

const REPORT: ContractReport = {
  findings: [
    {
      request: "orders/archive-order.yml",
      kind: "no-operation",
      operation: null,
      message: "no operation of the spec matches POST {{baseUrl}}/orders/:id/archive",
    },
    {
      request: "orders/list-orders.yml",
      kind: "missing-parameter",
      operation: "GET /orders",
      message: "required query parameter `region` is not sent",
    },
    {
      request: "orders/list-orders.yml",
      kind: "unknown-parameter",
      operation: "GET /orders",
      message: "query parameter `sort` is not declared by GET /orders",
    },
    {
      request: "orders/create-order.yml",
      kind: "body-mismatch",
      operation: "POST /orders",
      message: "body $: required property `currency` is missing",
    },
  ],
  uncovered: ["GET /orders/{id}/items", "DELETE /orders/{id}"],
  operations: 6,
  covered: 4,
};

const CHANGES: SpecChange[] = [
  {
    kind: "operation-removed",
    breaking: true,
    operation: "DELETE /orders/{id}",
    location: "",
    message: "operation removed",
    requests: ["orders/delete-order.yml"],
  },
  {
    kind: "required-added",
    breaking: true,
    operation: "GET /orders",
    location: "query parameter region",
    message: "new required query parameter region",
    requests: ["orders/list-orders.yml"],
  },
  {
    kind: "field-removed",
    breaking: true,
    operation: "GET /orders/{id}",
    location: "response 200: total",
    message: "field `total` removed",
    requests: ["orders/get-order.yml"],
  },
  {
    kind: "operation-added",
    breaking: false,
    operation: "GET /orders/{id}/items",
    location: "",
    message: "operation added",
    requests: [],
  },
  {
    kind: "field-added",
    breaking: false,
    operation: "POST /orders",
    location: "request body: coupon",
    message: "new field `coupon`",
    requests: ["orders/create-order.yml"],
  },
];

function render(title: string, changes: SpecChange[]): string {
  const line = (c: SpecChange) =>
    `- \`${c.operation}\`: ${c.message}` +
    (c.requests.length ? ` (affects ${c.requests.map((r) => `\`${r}\``).join(", ")})` : "") +
    "\n";
  const breaking = changes.filter((c) => c.breaking);
  const other = changes.filter((c) => !c.breaking);
  let out = `# ${title}\n\n## Breaking changes\n\n`;
  out += breaking.length ? breaking.map(line).join("") : "None.\n";
  if (other.length) out += `\n## Other changes\n\n${other.map(line).join("")}`;
  return out;
}

/** The pull request number of a commit summary, like `rt_app::contract::pull_number`. */
function pullNumber(summary: string): number | null {
  const m = /^Merge pull request #(\d+)/.exec(summary) ?? /\(#(\d+)\)$/.exec(summary);
  return m ? Number(m[1]) : null;
}

export class MockContract implements ContractApi {
  readonly calls: string[] = [];
  private demo = false;

  constructor(private readonly host: MockWave12Host) {
    if (typeof location !== "undefined" && new URLSearchParams(location.search).get("contract") === "1") {
      this.demo = true;
    }
  }

  enableDemo(on = true): void {
    this.demo = on;
  }

  async spec(): Promise<ContractSpec | null> {
    this.calls.push("spec");
    return this.demo ? { ...SPEC } : null;
  }

  async check(rev: string | null): Promise<ContractReport> {
    this.calls.push(`check ${rev ?? ""}`);
    this.need();
    if (rev !== null) await this.host.git().revParse(rev);
    return structuredClone(REPORT);
  }

  async changes(from: string, to: string): Promise<SpecChanges> {
    this.calls.push(`changes ${from}..${to}`);
    this.need();
    await this.host.git().revParse(from);
    await this.host.git().revParse(to);
    return { from_version: "1.1.0", to_version: SPEC.version, changes: structuredClone(CHANGES) };
  }

  async changelog(from: string, to: string): Promise<string> {
    const c = await this.changes(from, to);
    return render(`${SPEC.title} ${c.from_version} → ${c.to_version}`, c.changes);
  }

  /** Wave 17, strand A: writes the spec, unsaved (as a companion file, so `dirty` and `save`
   *  see it). Like the core it refuses a text that is no spec; the mock's check is a plain
   *  look for the `openapi` and `info` keys. */
  async stageSpec(text: string): Promise<string> {
    this.calls.push("stageSpec");
    this.need();
    const looksLikeSpec = /^\s*(openapi|"openapi")\s*:/m.test(text) && /^\s*(info|"info")\s*:/m.test(text);
    if (!looksLikeSpec || text.length > 16 * 1024 * 1024) {
      throw new BackendError("format", "the text is not an OpenAPI spec");
    }
    await this.host.backend.companions.stage(SPEC.path, text);
    return SPEC.path;
  }

  /** Wave 16, strand D: the changelog and its parts, with the commits of the mock history. */
  async changelogFacts(from: string, to: string): Promise<ChangelogFacts> {
    const c = await this.changes(from, to);
    const title = `${SPEC.title} ${c.from_version} → ${c.to_version}`;
    const commits = (await this.host.git().commits(from, to, 1000)).map((x) => {
      const pull = pullNumber(x.summary);
      return { oid: x.oid, summary: x.summary, author: x.author, ...(pull === null ? {} : { pull }) };
    });
    return {
      from,
      to,
      title,
      spec: c,
      collection: { changes: [], files: [] },
      commits,
      markdown: render(title, c.changes),
    };
  }

  private need(): void {
    if (!this.demo) throw new BackendError("format", "No OpenAPI spec found.");
  }
}

export function mockContract(host: MockWave12Host): MockContract {
  return new MockContract(host);
}
