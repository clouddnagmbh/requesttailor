// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { companionHttp, hostOf, toBase64 } from "./http";

const companion = { url: "http://127.0.0.1:47830/", token: "tok" };

function fakeFetch(answer: (url: string, init: RequestInit) => Response) {
  const calls: { url: string; init: RequestInit }[] = [];
  const f = (async (url: string, init: RequestInit) => {
    calls.push({ url, init });
    return answer(url, init);
  }) as unknown as typeof fetch;
  return { f, calls };
}

async function read(body: AsyncIterableIterator<Uint8Array>): Promise<string> {
  let out = "";
  for await (const c of body) out += new TextDecoder().decode(c);
  return out;
}

const head = (status: number, headers: [string, string][], url = "https://git.example/r.git/info/refs") =>
  JSON.stringify({ status, version: "HTTP/1.1", headers, url, redirects: [], timing: {} });

describe("companionHttp", () => {
  it("wraps the Git request into a WireRequest and unwraps the upstream response", async () => {
    const { f, calls } = fakeFetch(
      () =>
        new Response("001e# service=git-upload-pack\n", {
          status: 200,
          headers: {
            "x-rt-head": head(200, [
              ["Content-Type", "application/x-git-upload-pack-advertisement"],
              ["Set-Cookie", "a=1"],
              ["set-cookie", "b=2"],
            ]),
          },
        }),
    );
    const http = companionHttp(companion, f);
    async function* body() {
      yield new TextEncoder().encode("00");
      yield new TextEncoder().encode("00");
    }
    const res = await http.request({
      url: "https://git.example/r.git/git-upload-pack",
      method: "POST",
      headers: { "content-type": "application/x-git-upload-pack-request", authorization: "Basic eDp5" },
      body: body(),
    });
    expect(calls).toHaveLength(1);
    expect(calls[0].url).toBe("http://127.0.0.1:47830/v1/send");
    expect((calls[0].init.headers as Record<string, string>).Authorization).toBe("Bearer tok");
    const wire = JSON.parse(String(calls[0].init.body));
    expect(wire).toEqual({
      method: "POST",
      url: "https://git.example/r.git/git-upload-pack",
      headers: [
        ["content-type", "application/x-git-upload-pack-request"],
        ["authorization", "Basic eDp5"],
      ],
      body: btoa("0000"),
      content_type: null,
      timeout_ms: null,
      follow_redirects: true,
      max_redirects: 10,
    });
    expect(res.statusCode).toBe(200);
    expect(res.statusMessage).toBe("OK");
    expect(res.url).toBe("https://git.example/r.git/info/refs");
    expect(res.headers["content-type"]).toBe("application/x-git-upload-pack-advertisement");
    expect(res.headers["set-cookie"]).toBe("a=1, b=2");
    expect(await read(res.body)).toBe("001e# service=git-upload-pack\n");
  });

  it("passes the upstream status through (401 lets isomorphic-git ask for credentials)", async () => {
    const { f, calls } = fakeFetch(
      () => new Response("", { status: 200, headers: { "x-rt-head": head(401, [["WWW-Authenticate", "Basic"]]) } }),
    );
    const res = await companionHttp(companion, f).request({ url: "https://git.example/r.git/info/refs" });
    expect(res.statusCode).toBe(401);
    expect(JSON.parse(String(calls[0].init.body)).body).toBeNull();
  });

  it("says how to start the companion when it is not running", async () => {
    const f = (async () => {
      throw new TypeError("Failed to fetch");
    }) as unknown as typeof fetch;
    await expect(companionHttp(companion, f).request({ url: "https://x/r" })).rejects.toMatchObject({
      kind: "unsupported",
      message: expect.stringContaining("Start `rt agent`"),
    });
  });

  it("maps a refused token and transport failures", async () => {
    const refused = fakeFetch(() => new Response("pairing required", { status: 401 }));
    await expect(companionHttp(companion, refused.f).request({ url: "https://x/r" })).rejects.toMatchObject({
      kind: "companion",
    });
    const failed = fakeFetch(
      () => new Response(JSON.stringify({ kind: "connect", message: "connection refused" }), { status: 502 }),
    );
    await expect(
      companionHttp(companion, failed.f).request({ url: "https://git.example:8443/r" }),
    ).rejects.toMatchObject({ kind: "git", message: "Could not reach git.example:8443: connection refused" });
  });

  it("encodes large bodies as base64", () => {
    const big = new Uint8Array(100_000).map((_, i) => i % 251);
    const decoded = Uint8Array.from(atob(toBase64(big)), (c) => c.charCodeAt(0));
    expect(decoded).toEqual(big);
    expect(hostOf("https://user@git.example:8443/a.git")).toBe("git.example:8443");
  });
});
