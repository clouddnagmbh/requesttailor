// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `require('crypto-js')`: the part of the crypto-js API that request scripts use, written for
// RequestTailor (not the crypto-js code): hashes (MD5, SHA-1, SHA-224, SHA-256, SHA-384,
// SHA-512), HMAC with each of them, progressive hashing (`CryptoJS.algo.SHA256.create()`),
// PBKDF2, `WordArray` and the encoders Hex, Base64, Base64url, Utf8, Latin1. Ciphers (AES,
// DES, …) are not included. Values and results match crypto-js 4.
//
// Standard ECMAScript only; `WordArray.random` uses Math.random (not for keys).

"use strict";

// ---------------------------------------------------------------------------------------
// WordArray: big-endian 32-bit words, `sigBytes` significant bytes (as crypto-js)

function WordArray(words, sigBytes) {
  this.words = words || [];
  this.sigBytes = sigBytes !== undefined ? sigBytes : this.words.length * 4;
}

WordArray.prototype.toString = function (encoder) {
  return (encoder || Hex).stringify(this);
};
WordArray.prototype.concat = function (other) {
  const bytes = toBytes(this).concat(toBytes(other));
  const w = fromBytes(bytes);
  this.words = w.words;
  this.sigBytes = w.sigBytes;
  return this;
};
WordArray.prototype.clamp = function () {
  const w = fromBytes(toBytes(this));
  this.words = w.words;
  this.sigBytes = w.sigBytes;
  return this;
};
WordArray.prototype.clone = function () {
  return new WordArray(this.words.slice(), this.sigBytes);
};

WordArray.create = function (words, sigBytes) {
  if (words instanceof ArrayBuffer) words = new Uint8Array(words);
  if (ArrayBuffer.isView(words)) return fromBytes(Array.from(new Uint8Array(words.buffer, words.byteOffset, words.byteLength)));
  return new WordArray(words, sigBytes);
};
WordArray.random = function (n) {
  const bytes = [];
  for (let i = 0; i < n; i++) bytes.push(Math.floor(Math.random() * 256));
  return fromBytes(bytes);
};

function toBytes(wa) {
  const out = [];
  for (let i = 0; i < wa.sigBytes; i++) out.push((wa.words[i >>> 2] >>> (24 - (i % 4) * 8)) & 0xff);
  return out;
}

function fromBytes(bytes) {
  const words = [];
  for (let i = 0; i < bytes.length; i++) words[i >>> 2] |= (bytes[i] & 0xff) << (24 - (i % 4) * 8);
  for (let i = 0; i < words.length; i++) words[i] |= 0;
  return new WordArray(words, bytes.length);
}

// ---------------------------------------------------------------------------------------
// Encoders

const Hex = {
  stringify: (wa) => toBytes(wa).map((b) => (b < 16 ? "0" : "") + b.toString(16)).join(""),
  parse(str) {
    const bytes = [];
    for (let i = 0; i + 1 < str.length; i += 2) bytes.push(parseInt(str.substr(i, 2), 16));
    return fromBytes(bytes);
  },
};

const Latin1 = {
  stringify: (wa) => toBytes(wa).map((b) => String.fromCharCode(b)).join(""),
  parse(str) {
    const bytes = [];
    for (let i = 0; i < str.length; i++) bytes.push(str.charCodeAt(i) & 0xff);
    return fromBytes(bytes);
  },
};

function utf8Encode(str) {
  const out = [];
  for (let i = 0; i < str.length; i++) {
    let c = str.charCodeAt(i);
    if (c >= 0xd800 && c < 0xdc00 && i + 1 < str.length) {
      const d = str.charCodeAt(i + 1);
      if (d >= 0xdc00 && d < 0xe000) {
        c = 0x10000 + ((c - 0xd800) << 10) + (d - 0xdc00);
        i++;
      }
    }
    if (c < 0x80) out.push(c);
    else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 63));
    else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
    else out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
  }
  return out;
}

function utf8Decode(bytes) {
  let s = "";
  for (let i = 0; i < bytes.length; ) {
    const b = bytes[i++];
    let c;
    if (b < 0x80) c = b;
    else if (b < 0xe0) c = ((b & 31) << 6) | (bytes[i++] & 63);
    else if (b < 0xf0) c = ((b & 15) << 12) | ((bytes[i++] & 63) << 6) | (bytes[i++] & 63);
    else c = ((b & 7) << 18) | ((bytes[i++] & 63) << 12) | ((bytes[i++] & 63) << 6) | (bytes[i++] & 63);
    s += String.fromCodePoint(c);
  }
  return s;
}

const Utf8 = {
  stringify(wa) {
    const bytes = toBytes(wa);
    const s = utf8Decode(bytes);
    if (/�/.test(s) && !/�/.test(Latin1.stringify(wa))) throw new Error("Malformed UTF-8 data");
    return s;
  },
  parse: (str) => fromBytes(utf8Encode(String(str))),
};

const Utf16BE = {
  stringify(wa) {
    const b = toBytes(wa);
    let s = "";
    for (let i = 0; i + 1 < b.length; i += 2) s += String.fromCharCode((b[i] << 8) | b[i + 1]);
    return s;
  },
  parse(str) {
    const b = [];
    for (let i = 0; i < str.length; i++) b.push(str.charCodeAt(i) >> 8, str.charCodeAt(i) & 255);
    return fromBytes(b);
  },
};
const Utf16LE = {
  stringify(wa) {
    const b = toBytes(wa);
    let s = "";
    for (let i = 0; i + 1 < b.length; i += 2) s += String.fromCharCode(b[i] | (b[i + 1] << 8));
    return s;
  },
  parse(str) {
    const b = [];
    for (let i = 0; i < str.length; i++) b.push(str.charCodeAt(i) & 255, str.charCodeAt(i) >> 8);
    return fromBytes(b);
  },
};

function base64(alphabet, pad) {
  return {
    stringify(wa) {
      const b = toBytes(wa);
      let out = "";
      for (let i = 0; i < b.length; i += 3) {
        const n = (b[i] << 16) | ((b[i + 1] || 0) << 8) | (b[i + 2] || 0);
        out += alphabet[n >> 18] + alphabet[(n >> 12) & 63];
        out += i + 1 < b.length ? alphabet[(n >> 6) & 63] : pad ? "=" : "";
        out += i + 2 < b.length ? alphabet[n & 63] : pad ? "=" : "";
      }
      return out;
    },
    parse(str) {
      const bytes = [];
      let bits = 0;
      let n = 0;
      for (const ch of String(str)) {
        const v = alphabet.indexOf(ch);
        if (v < 0) continue;
        n = ((n << 6) | v) & 0xffffff;
        bits += 6;
        if (bits >= 8) {
          bits -= 8;
          bytes.push((n >> bits) & 255);
        }
      }
      return fromBytes(bytes);
    },
  };
}

const Base64 = base64("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/", true);
const Base64url = base64("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_", false);

/** Bytes of a message: a string is UTF-8, a WordArray as it is. */
function bytesOf(message) {
  if (message instanceof WordArray || (message && Array.isArray(message.words) && typeof message.sigBytes === "number")) return toBytes(message);
  if (message === undefined || message === null) return [];
  return utf8Encode(String(message));
}

// ---------------------------------------------------------------------------------------
// Hash functions on byte arrays

function rotl(x, n) {
  return (x << n) | (x >>> (32 - n));
}

function padded(bytes, littleEndian, lengthBytes) {
  const m = bytes.slice();
  const bitLen = bytes.length * 8;
  m.push(0x80);
  const block = lengthBytes === 16 ? 128 : 64;
  while (m.length % block !== block - lengthBytes) m.push(0);
  const len = [];
  for (let i = 0; i < lengthBytes; i++) len.push(0);
  // Lengths up to 2^53 bits.
  let hi = Math.floor(bitLen / 0x100000000);
  let lo = bitLen >>> 0;
  for (let i = 0; i < 4; i++) {
    len[lengthBytes - 1 - i] = (lo >>> (8 * i)) & 255;
    len[lengthBytes - 5 - i] = (hi >>> (8 * i)) & 255;
  }
  if (littleEndian) len.reverse();
  return m.concat(len);
}

function md5(bytes) {
  const K = [];
  for (let i = 0; i < 64; i++) K[i] = Math.floor(Math.abs(Math.sin(i + 1)) * 0x100000000) | 0;
  const R = [7, 12, 17, 22, 5, 9, 14, 20, 4, 11, 16, 23, 6, 10, 15, 21];
  const m = padded(bytes, true, 8);
  let a0 = 0x67452301, b0 = 0xefcdab89 | 0, c0 = 0x98badcfe | 0, d0 = 0x10325476;
  for (let o = 0; o < m.length; o += 64) {
    const M = [];
    for (let i = 0; i < 16; i++) M[i] = m[o + i * 4] | (m[o + i * 4 + 1] << 8) | (m[o + i * 4 + 2] << 16) | (m[o + i * 4 + 3] << 24);
    let a = a0, b = b0, c = c0, d = d0;
    for (let i = 0; i < 64; i++) {
      let f, g;
      if (i < 16) (f = (b & c) | (~b & d)), (g = i);
      else if (i < 32) (f = (d & b) | (~d & c)), (g = (5 * i + 1) % 16);
      else if (i < 48) (f = b ^ c ^ d), (g = (3 * i + 5) % 16);
      else (f = c ^ (b | ~d)), (g = (7 * i) % 16);
      const tmp = d;
      d = c;
      c = b;
      b = (b + rotl((a + f + K[i] + M[g]) | 0, R[(i >> 4) * 4 + (i % 4)])) | 0;
      a = tmp;
    }
    a0 = (a0 + a) | 0;
    b0 = (b0 + b) | 0;
    c0 = (c0 + c) | 0;
    d0 = (d0 + d) | 0;
  }
  const out = [];
  for (const w of [a0, b0, c0, d0]) out.push(w & 255, (w >>> 8) & 255, (w >>> 16) & 255, (w >>> 24) & 255);
  return out;
}

function sha1(bytes) {
  const m = padded(bytes, false, 8);
  let h = [0x67452301, 0xefcdab89 | 0, 0x98badcfe | 0, 0x10325476, 0xc3d2e1f0 | 0];
  const w = new Array(80);
  for (let o = 0; o < m.length; o += 64) {
    for (let i = 0; i < 16; i++) w[i] = (m[o + i * 4] << 24) | (m[o + i * 4 + 1] << 16) | (m[o + i * 4 + 2] << 8) | m[o + i * 4 + 3];
    for (let i = 16; i < 80; i++) w[i] = rotl(w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16], 1);
    let [a, b, c, d, e] = h;
    for (let i = 0; i < 80; i++) {
      let f, k;
      if (i < 20) (f = (b & c) | (~b & d)), (k = 0x5a827999);
      else if (i < 40) (f = b ^ c ^ d), (k = 0x6ed9eba1);
      else if (i < 60) (f = (b & c) | (b & d) | (c & d)), (k = 0x8f1bbcdc | 0);
      else (f = b ^ c ^ d), (k = 0xca62c1d6 | 0);
      const t = (rotl(a, 5) + f + e + k + w[i]) | 0;
      e = d;
      d = c;
      c = rotl(b, 30);
      b = a;
      a = t;
    }
    h = [(h[0] + a) | 0, (h[1] + b) | 0, (h[2] + c) | 0, (h[3] + d) | 0, (h[4] + e) | 0];
  }
  return wordsToBytes(h);
}

function wordsToBytes(words) {
  const out = [];
  for (const w of words) out.push((w >>> 24) & 255, (w >>> 16) & 255, (w >>> 8) & 255, w & 255);
  return out;
}

const K256 = [
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
].map((x) => x | 0);

function sha256(bytes, is224) {
  const m = padded(bytes, false, 8);
  let h = (is224
    ? [0xc1059ed8, 0x367cd507, 0x3070dd17, 0xf70e5939, 0xffc00b31, 0x68581511, 0x64f98fa7, 0xbefa4fa4]
    : [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19]
  ).map((x) => x | 0);
  const w = new Array(64);
  const rotr = (x, n) => (x >>> n) | (x << (32 - n));
  for (let o = 0; o < m.length; o += 64) {
    for (let i = 0; i < 16; i++) w[i] = (m[o + i * 4] << 24) | (m[o + i * 4 + 1] << 16) | (m[o + i * 4 + 2] << 8) | m[o + i * 4 + 3];
    for (let i = 16; i < 64; i++) {
      const s0 = rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ (w[i - 15] >>> 3);
      const s1 = rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ (w[i - 2] >>> 10);
      w[i] = (w[i - 16] + s0 + w[i - 7] + s1) | 0;
    }
    let [a, b, c, d, e, f, g, hh] = h;
    for (let i = 0; i < 64; i++) {
      const S1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
      const ch = (e & f) ^ (~e & g);
      const t1 = (hh + S1 + ch + K256[i] + w[i]) | 0;
      const S0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
      const maj = (a & b) ^ (a & c) ^ (b & c);
      const t2 = (S0 + maj) | 0;
      hh = g;
      g = f;
      f = e;
      e = (d + t1) | 0;
      d = c;
      c = b;
      b = a;
      a = (t1 + t2) | 0;
    }
    h = [a, b, c, d, e, f, g, hh].map((x, i) => (h[i] + x) | 0);
  }
  return wordsToBytes(is224 ? h.slice(0, 7) : h);
}

const K512 = [
  "428a2f98d728ae22", "7137449123ef65cd", "b5c0fbcfec4d3b2f", "e9b5dba58189dbbc", "3956c25bf348b538",
  "59f111f1b605d019", "923f82a4af194f9b", "ab1c5ed5da6d8118", "d807aa98a3030242", "12835b0145706fbe",
  "243185be4ee4b28c", "550c7dc3d5ffb4e2", "72be5d74f27b896f", "80deb1fe3b1696b1", "9bdc06a725c71235",
  "c19bf174cf692694", "e49b69c19ef14ad2", "efbe4786384f25e3", "0fc19dc68b8cd5b5", "240ca1cc77ac9c65",
  "2de92c6f592b0275", "4a7484aa6ea6e483", "5cb0a9dcbd41fbd4", "76f988da831153b5", "983e5152ee66dfab",
  "a831c66d2db43210", "b00327c898fb213f", "bf597fc7beef0ee4", "c6e00bf33da88fc2", "d5a79147930aa725",
  "06ca6351e003826f", "142929670a0e6e70", "27b70a8546d22ffc", "2e1b21385c26c926", "4d2c6dfc5ac42aed",
  "53380d139d95b3df", "650a73548baf63de", "766a0abb3c77b2a8", "81c2c92e47edaee6", "92722c851482353b",
  "a2bfe8a14cf10364", "a81a664bbc423001", "c24b8b70d0f89791", "c76c51a30654be30", "d192e819d6ef5218",
  "d69906245565a910", "f40e35855771202a", "106aa07032bbd1b8", "19a4c116b8d2d0c8", "1e376c085141ab53",
  "2748774cdf8eeb99", "34b0bcb5e19b48a8", "391c0cb3c5c95a63", "4ed8aa4ae3418acb", "5b9cca4f7763e373",
  "682e6ff3d6b2b8a3", "748f82ee5defb2fc", "78a5636f43172f60", "84c87814a1f0ab72", "8cc702081a6439ec",
  "90befffa23631e28", "a4506cebde82bde9", "bef9a3f7b2c67915", "c67178f2e372532b", "ca273eceea26619c",
  "d186b8c721c0c207", "eada7dd6cde0eb1e", "f57d4f7fee6ed178", "06f067aa72176fba", "0a637dc5a2c898a6",
  "113f9804bef90dae", "1b710b35131c471b", "28db77f523047d84", "32caab7b40c72493", "3c9ebe0a15c9bebc",
  "431d67c49c100d4c", "4cc5d4becb3e42b6", "597f299cfc657e2a", "5fcb6fab3ad6faec", "6c44198c4a475817",
].map((h) => BigInt("0x" + h));

function sha512(bytes, is384) {
  const M64 = (1n << 64n) - 1n;
  const add = (...xs) => xs.reduce((a, b) => (a + b) & M64, 0n);
  const rotr = (x, n) => ((x >> BigInt(n)) | (x << BigInt(64 - n))) & M64;
  const m = padded(bytes, false, 16);
  let h = (is384
    ? ["cbbb9d5dc1059ed8", "629a292a367cd507", "9159015a3070dd17", "152fecd8f70e5939", "67332667ffc00b31", "8eb44a8768581511", "db0c2e0d64f98fa7", "47b5481dbefa4fa4"]
    : ["6a09e667f3bcc908", "bb67ae8584caa73b", "3c6ef372fe94f82b", "a54ff53a5f1d36f1", "510e527fade682d1", "9b05688c2b3e6c1f", "1f83d9abfb41bd6b", "5be0cd19137e2179"]
  ).map((x) => BigInt("0x" + x));
  const w = new Array(80);
  for (let o = 0; o < m.length; o += 128) {
    for (let i = 0; i < 16; i++) {
      let x = 0n;
      for (let k = 0; k < 8; k++) x = (x << 8n) | BigInt(m[o + i * 8 + k]);
      w[i] = x;
    }
    for (let i = 16; i < 80; i++) {
      const s0 = rotr(w[i - 15], 1) ^ rotr(w[i - 15], 8) ^ (w[i - 15] >> 7n);
      const s1 = rotr(w[i - 2], 19) ^ rotr(w[i - 2], 61) ^ (w[i - 2] >> 6n);
      w[i] = add(w[i - 16], s0, w[i - 7], s1);
    }
    let [a, b, c, d, e, f, g, hh] = h;
    for (let i = 0; i < 80; i++) {
      const S1 = rotr(e, 14) ^ rotr(e, 18) ^ rotr(e, 41);
      const ch = (e & f) ^ (~e & M64 & g);
      const t1 = add(hh, S1, ch, K512[i], w[i]);
      const S0 = rotr(a, 28) ^ rotr(a, 34) ^ rotr(a, 39);
      const maj = (a & b) ^ (a & c) ^ (b & c);
      const t2 = add(S0, maj);
      hh = g;
      g = f;
      f = e;
      e = add(d, t1);
      d = c;
      c = b;
      b = a;
      a = add(t1, t2);
    }
    h = [a, b, c, d, e, f, g, hh].map((x, i) => add(h[i], x));
  }
  const out = [];
  for (const x of is384 ? h.slice(0, 6) : h) for (let k = 7; k >= 0; k--) out.push(Number((x >> BigInt(8 * k)) & 255n));
  return out;
}

const HASHES = {
  MD5: { fn: md5, block: 64 },
  SHA1: { fn: sha1, block: 64 },
  SHA224: { fn: (b) => sha256(b, true), block: 64 },
  SHA256: { fn: (b) => sha256(b, false), block: 64 },
  SHA384: { fn: (b) => sha512(b, true), block: 128 },
  SHA512: { fn: (b) => sha512(b, false), block: 128 },
};

function hmacBytes(hash, keyBytes, msgBytes) {
  let key = keyBytes;
  if (key.length > hash.block) key = hash.fn(key);
  const k = key.concat(new Array(hash.block - key.length).fill(0));
  const inner = hash.fn(k.map((b) => b ^ 0x36).concat(msgBytes));
  return hash.fn(k.map((b) => b ^ 0x5c).concat(inner));
}

// ---------------------------------------------------------------------------------------

const C = {
  lib: { WordArray },
  enc: { Hex, Latin1, Utf8, Base64, Base64url, Utf16: Utf16BE, Utf16BE, Utf16LE },
  algo: {},
};

for (const [name, hash] of Object.entries(HASHES)) {
  C[name] = (message) => fromBytes(hash.fn(bytesOf(message)));
  C["Hmac" + name] = (message, key) => fromBytes(hmacBytes(hash, bytesOf(key), bytesOf(message)));
  const algo = {
    create() {
      let buf = [];
      const h = {
        update(m) {
          buf = buf.concat(bytesOf(m));
          return h;
        },
        finalize(m) {
          if (m !== undefined) h.update(m);
          const out = fromBytes(hash.fn(buf));
          buf = [];
          return out;
        },
        reset() {
          buf = [];
          return h;
        },
      };
      return h;
    },
  };
  Object.defineProperty(algo, "__hash", { value: hash });
  C.algo[name] = algo;
}

C.algo.HMAC = {
  create(algo, key) {
    const hash = algo.__hash;
    const k = bytesOf(key);
    let buf = [];
    const h = {
      update(m) {
        buf = buf.concat(bytesOf(m));
        return h;
      },
      finalize(m) {
        if (m !== undefined) h.update(m);
        const out = fromBytes(hmacBytes(hash, k, buf));
        buf = [];
        return out;
      },
      reset() {
        buf = [];
        return h;
      },
    };
    return h;
  },
};

/** PBKDF2 (`keySize` in 32-bit words, default 4; `hasher` default SHA256 as crypto-js 4.2). */
C.PBKDF2 = function (password, salt, cfg) {
  cfg = cfg || {};
  const hash = (cfg.hasher || C.algo.SHA256).__hash;
  const keyBytes = (cfg.keySize === undefined ? 4 : cfg.keySize) * 4;
  const iterations = cfg.iterations === undefined ? 250000 : cfg.iterations;
  const p = bytesOf(password);
  const s = bytesOf(salt);
  const out = [];
  for (let block = 1; out.length < keyBytes; block++) {
    let u = hmacBytes(hash, p, s.concat([(block >>> 24) & 255, (block >>> 16) & 255, (block >>> 8) & 255, block & 255]));
    const t = u.slice();
    for (let i = 1; i < iterations; i++) {
      u = hmacBytes(hash, p, u);
      for (let j = 0; j < t.length; j++) t[j] ^= u[j];
    }
    out.push(...t);
  }
  return fromBytes(out.slice(0, keyBytes));
};

for (const cipher of ["AES", "DES", "TripleDES", "Rabbit", "RC4", "SHA3", "RIPEMD160"]) {
  C[cipher] = new Proxy(function () {}, {
    get() {
      throw new Error("CryptoJS." + cipher + " is not supported in RequestTailor; see SHIM-COVERAGE.md");
    },
    apply() {
      throw new Error("CryptoJS." + cipher + " is not supported in RequestTailor; see SHIM-COVERAGE.md");
    },
  });
}

module.exports = C;
