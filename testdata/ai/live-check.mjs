// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Manual check of the real provider endpoints. Off unless RT_AI_LIVE=1 and a key is set:
//   RT_AI_LIVE=1 RT_AI_LIVE_ANTHROPIC_KEY=... node testdata/ai/live-check.mjs
// It sends one tiny prompt per provider with a key and prints the HTTP status. Never run in CI.

if (process.env.RT_AI_LIVE !== "1") {
  console.log("RT_AI_LIVE is not 1: nothing to do.");
  process.exit(0);
}

const prompt = "Reply with the word ok.";
const targets = {
  ANTHROPIC: (k, m) => ["https://api.anthropic.com/v1/messages", { "x-api-key": k, "anthropic-version": "2023-06-01" }, { model: m, max_tokens: 16, messages: [{ role: "user", content: prompt }] }],
  OPENAI: (k, m) => ["https://api.openai.com/v1/chat/completions", { authorization: `Bearer ${k}` }, { model: m, messages: [{ role: "user", content: prompt }] }],
  MISTRAL: (k, m) => ["https://api.mistral.ai/v1/chat/completions", { authorization: `Bearer ${k}` }, { model: m, messages: [{ role: "user", content: prompt }] }],
  GEMINI: (k, m) => [`https://generativelanguage.googleapis.com/v1beta/models/${m}:generateContent`, { "x-goog-api-key": k }, { contents: [{ role: "user", parts: [{ text: prompt }] }] }],
};

for (const [name, make] of Object.entries(targets)) {
  const key = process.env[`RT_AI_LIVE_${name}_KEY`];
  const model = process.env[`RT_AI_LIVE_${name}_MODEL`];
  if (!key || !model) continue;
  const [url, headers, body] = make(key, model);
  const r = await fetch(url, { method: "POST", headers: { "content-type": "application/json", ...headers }, body: JSON.stringify(body) });
  console.log(name, r.status);
}
