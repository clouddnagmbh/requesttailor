// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand B: the one-time question and `ExtensionContext.egress` over the mock backend,
// which follows the rules of `rt_app::egress` (hosts allowed per device, `not-allowed`,
// `blocked`, secrets named by reference only).

import { describe, expect, it } from "vitest";

import { BackendError } from "../commands/backend";
import { MockBackend } from "../commands/mock";
import type { EgressRequest } from "../commands/types";
import { AppState } from "../state/app.svelte";
import { askEgress, hostLabel, secretNames } from "./ask";
import { createExtensionEgress } from "./extension-egress";

function call(url: string, headers: [string, string][] = []): EgressRequest {
  return { method: "GET", url, headers, extension: "traces", purpose: "Server traces" };
}

function setup() {
  const backend = new MockBackend();
  const app = new AppState(backend);
  return { backend, app, egress: createExtensionEgress(backend, app) };
}

describe("the question", () => {
  it("names the extension, host and purpose, and carries the secret names, never values", async () => {
    const { app } = setup();
    const answer = askEgress(app, call("https://traces.example/api", [["Authorization", "Bearer {{secret:token}}"]]));
    const shown = app.dialog!.dialog;
    expect(shown.title).toBe("Allow network access?");
    expect(shown.props).toMatchObject({ extension: "traces", purpose: "Server traces", host: "traces.example", port: null, secrets: ["token"] });
    app.closeDialog(true);
    expect(await answer).toBe(true);
  });

  it("is a no when closed or declined, and for a URL that is not one", async () => {
    const { app } = setup();
    const declined = askEgress(app, call("https://traces.example/"));
    app.closeDialog(false);
    expect(await declined).toBe(false);
    const closed = askEgress(app, call("https://traces.example/"));
    app.closeDialog();
    expect(await closed).toBe(false);
    expect(await askEgress(app, call("not a url"))).toBe(false);
    expect(app.dialog).toBeNull();
  });

  it("shows plain http on its default port as port 80", () => {
    const { app } = setup();
    void askEgress(app, call("http://tempo.example/x"));
    expect(app.dialog!.dialog.props).toMatchObject({ host: "tempo.example", port: 80 });
    expect(hostLabel("tempo.example", 80)).toBe("tempo.example:80");
    expect(hostLabel("tempo.example", null)).toBe("tempo.example");
  });

  it("lists each secret name once", () => {
    expect(secretNames(call("https://a/", [["A", "{{secret:x}}"], ["B", "k={{ secret:x }}&{{secret:y}}"]]))).toEqual(["x", "y"]);
  });
});

describe("ExtensionContext.egress", () => {
  it("asks once per host: the call fails, the user allows, the call is repeated and later calls do not ask", async () => {
    const { backend, app, egress } = setup();
    const first = egress.request(call("https://traces.example/api/t/1", [["Authorization", "Bearer {{secret:token}}"]]));
    await expect.poll(() => app.dialog !== null).toBe(true);
    app.closeDialog(true);
    const answer = await first;
    expect(answer.status).toBe(200);
    // The answer says which secret was used and never what it was.
    expect(answer.body).toContain("token=••••");
    expect((await backend.egress.hosts()).map((h) => [h.host, h.extension, h.purpose])).toEqual([["traces.example", "traces", "Server traces"]]);
    // The second call goes through without a question.
    await egress.request(call("https://traces.example/api/t/2"));
    expect(app.dialog).toBeNull();
    // Another host asks again.
    const other = egress.request(call("https://other.example/"));
    await expect.poll(() => app.dialog !== null).toBe(true);
    app.closeDialog(false);
    await expect(other).rejects.toMatchObject({ kind: "not-allowed" });
    expect((await egress.hosts()).length).toBe(1);
  });

  it("does not ask for a host that is blocked: the guard's answer comes back as it is", async () => {
    const { backend, egress } = setup();
    await backend.egress.allow("blocked.example", null, "traces", "Server traces");
    const error = await egress.request(call("https://blocked.example/")).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(BackendError);
    expect((error as BackendError).kind).toBe("blocked");
  });

  it("allows plain http only on its own port entry, and removing a host takes it back", async () => {
    const { backend, app, egress } = setup();
    const first = egress.request(call("http://localhost:48706/x"));
    await expect.poll(() => app.dialog !== null).toBe(true);
    app.closeDialog(true);
    await first;
    expect((await backend.egress.hosts())[0]).toMatchObject({ host: "localhost", port: 48706 });
    await backend.egress.remove("localhost", 48706);
    expect(await backend.egress.hosts()).toEqual([]);
    const again = egress.request(call("http://localhost:48706/x"));
    await expect.poll(() => app.dialog !== null).toBe(true);
    app.closeDialog(false);
    await expect(again).rejects.toMatchObject({ kind: "not-allowed" });
  });
});
