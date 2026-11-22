// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { packImport } from "./import";

/** The inverse of `packImport`, as src-tauri/src/import.rs reads it. */
function unpack(body: Uint8Array) {
  const view = new DataView(body.buffer, body.byteOffset, body.byteLength);
  const headerLength = view.getUint32(0, true);
  const header = JSON.parse(new TextDecoder().decode(body.subarray(4, 4 + headerLength))) as {
    format: string | null;
    files: { name: string; size: number }[];
  };
  let at = 4 + headerLength;
  const files = header.files.map((f) => {
    const data = body.subarray(at, at + f.size);
    at += f.size;
    return { name: f.name, data: Array.from(data) };
  });
  return { format: header.format, files, rest: body.byteLength - at };
}

describe("packImport", () => {
  it("packs several files with their names and the format", () => {
    const body = packImport(
      [
        { name: "shop.postman_collection.json", data: new TextEncoder().encode("{}") },
        { name: "Prod – ünïcode.postman_environment.json", data: new Uint8Array([0, 255, 7]) },
      ],
      "postman",
    );
    expect(unpack(body)).toEqual({
      format: "postman",
      files: [
        { name: "shop.postman_collection.json", data: [123, 125] },
        { name: "Prod – ünïcode.postman_environment.json", data: [0, 255, 7] },
      ],
      rest: 0,
    });
  });

  it("keeps a detected format as null and handles empty input", () => {
    expect(unpack(packImport([], null))).toEqual({ format: null, files: [], rest: 0 });
  });
});
