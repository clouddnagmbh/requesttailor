// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { escapeMermaid, mermaidOf, mockSequence } from "../commands/mock-sequence";
import { describeSelection, moveSelection, omittedText, savedName, sourceChoices } from "./model";

describe("sourceChoices", () => {
  it("offers the pages of a request only with a request in hand", () => {
    expect(sourceChoices(null).map((c) => c.id)).toEqual(["run", "session", "mcp", "history"]);
    const withRequest = sourceChoices("items.yml");
    expect(withRequest.map((c) => c.id)).toEqual(["run", "session", "pages", "mcp", "history"]);
    expect(withRequest[2]?.source).toEqual({ kind: "pages", request_path: "items.yml" });
    expect(withRequest[0]?.source).toEqual({ kind: "run", run_id: 0 });
  });
});

describe("keyboard", () => {
  it("moves between messages and stays inside", () => {
    expect(moveSelection(null, 1, 4)).toBe(0);
    expect(moveSelection(null, -1, 4)).toBe(3);
    expect(moveSelection(3, 1, 4)).toBe(3);
    expect(moveSelection(1, -5, 4)).toBe(0);
    expect(moveSelection(null, 1, 0)).toBeNull();
  });

  it("says which message is selected", async () => {
    const api = mockSequence();
    const model = await api.model({ kind: "session" }, {});
    expect(describeSelection(model, null)).toContain("6 messages");
    expect(describeSelection(model, 2)).toBe("Message 3 of 6: request from Client to api.example.com, GET /orders. Enter opens the request.");
  });
});

describe("file name and notes", () => {
  it("names a saved diagram under sequence/", () => {
    expect(savedName("pages", new Date(2026, 10, 25, 9, 8, 7))).toBe("sequence/pages-20261125-090807.md");
    expect(savedName("Latest run!", new Date(2026, 0, 2, 3, 4, 5))).toBe("sequence/latest-run--20260102-030405.md");
  });

  it("explains left-out messages", () => {
    expect(omittedText({ participants: [], messages: [] })).toBeNull();
    expect(omittedText({ participants: [], messages: [], omitted: 12 })).toContain("12 more messages");
  });
});

describe("the mock's Mermaid copy", () => {
  it("escapes like the core", () => {
    expect(escapeMermaid("a;b#c:d%e<f>`g`")).toBe("a#59;b#35;c#58;d#37;e#lt;f#gt;#96;g#96;");
    expect(escapeMermaid("  ")).toBe("-");
    expect(escapeMermaid("x".repeat(100))).toBe(`${"x".repeat(80)}…`);
    expect(escapeMermaid("a\nb\tc")).toBe("a b c");
  });

  it("renders participants, arrows and notes", async () => {
    const api = mockSequence();
    const model = await api.model({ kind: "session" }, {});
    const text = mermaidOf(model);
    expect(text.startsWith("sequenceDiagram\n    participant P0 as Client\n")).toBe(true);
    expect(text).toContain("P0->>P2: POST /token");
    expect(text).toContain("P2-->>P0: 200 (38 ms)");
    expect(text).toContain("Note over P0: 1 of 2 tests failed#58; finds the order");
    const out = await api.render(model, {});
    expect(out.markdown).toBe(`\`\`\`mermaid\n${out.mermaid.trimEnd()}\n\`\`\`\n`);
  });
});
