// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { BackendError, type StreamEvent, type StreamLog } from "@requesttailor/ui";
import { describe, expect, it } from "vitest";

import { webStreams, type CoreStreamHandle, type WebStreamsHost } from "./streams";
import type { Session } from "./web-backend";

function setup(openStream: (...args: unknown[]) => Promise<CoreStreamHandle>) {
  const calls: unknown[][] = [];
  const host: WebStreamsHost = {
    session: () =>
      ({
        ws: {
          openStream: (...args: unknown[]) => {
            calls.push(args);
            return openStream(...args);
          },
        },
      }) as unknown as Session,
    sendContext: async (input) => ({
      input: { ...input, secrets: [["token", "s3cret"]] },
      companion: { url: "http://127.0.0.1:47881", token: "t" },
      services: null,
    }),
  };
  return { api: webStreams(host), calls };
}

function fakeHandle() {
  const done: string[] = [];
  const log: StreamLog = {
    version: 1,
    request_path: "ws.yml",
    environment: null,
    kind: "websocket",
    url: "ws://x",
    events: [],
  };
  const handle: CoreStreamHandle = {
    send: (m) => void done.push(`send ${m.data}`),
    finish: () => void done.push("finish"),
    close: () => void done.push("close"),
    log: () => log,
    ended: false,
    release: () => void done.push("release"),
    free: () => void done.push("free"),
  };
  return { handle, done };
}

describe("webStreams", () => {
  it("opens with the send context and forwards to the handle", async () => {
    const { handle, done } = fakeHandle();
    const { api, calls } = setup(async () => handle);
    const events: StreamEvent[] = [];
    const id = await api.open("ws.yml", { environment: "Local", vars: [] }, (e) => events.push(e));
    const [path, input, companion, services] = calls[0];
    expect(path).toBe("ws.yml");
    expect(input).toEqual({ environment: "Local", vars: [], secrets: [["token", "s3cret"]] });
    expect(companion).toEqual({ url: "http://127.0.0.1:47881", token: "t" });
    expect(services).toBeNull();

    await api.send(id, { title: "", selected: true, kind: "text", data: "hi" });
    await api.finish(id);
    await api.close(id);
    expect((await api.log(id)).kind).toBe("websocket");
    await api.release(id);
    expect(done).toEqual(["send hi", "finish", "close", "release", "free"]);
    await expect(api.log(id)).rejects.toBeInstanceOf(BackendError);
    await api.release(id);
  });

  it("rejects with the core's AppError kind", async () => {
    const { api } = setup(async () => {
      const e = new Error("request not found: x.yml");
      Object.assign(e, { kind: "request-not-found" });
      throw e;
    });
    const err = await api.open("x.yml", { environment: null, vars: [] }, () => {}).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(BackendError);
    expect(err).toMatchObject({ kind: "request-not-found" });
  });
});
