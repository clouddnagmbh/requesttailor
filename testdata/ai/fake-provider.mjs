// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// A fake AI provider for tests: the wire formats of the providers of the matrix under their own
// paths, with streaming and tool use. No dependencies, loopback only. Never a real call.
//
//   node testdata/ai/fake-provider.mjs [port]      port: RT_AI_FAKE_PORT, else 48330
//
// Paths (all POST):
//   /anthropic/v1/messages                       Messages API
//   /openai/v1/chat/completions                  OpenAI
//   /azure/openai/v1/chat/completions            Azure OpenAI (key in `api-key`)
//   /mistral/v1/chat/completions                 Mistral
//   /compat/v1/chat/completions                  an OpenAI-compatible server (key optional)
//   /gemini/v1beta/models/<m>:generateContent    Gemini (and `:streamGenerateContent`)
//
// Control (loopback only; `/__` paths have CORS on):
//   POST /__script   {"replies":[{text, toolCalls:[{id?,name,input}], stop?, status?, error?,
//                     delayMs?}, ...]}  the next answers, in order (the last repeats)
//   GET  /__calls    [{path, headers, body, stream}] everything received, in order
//   POST /__reset    forget script and calls
//   POST /__cors     {"anthropic": true, ...}  CORS per path prefix (default: what the real
//                    provider allows: anthropic and gemini on, the rest off)

import http from "node:http";

const port = Number(process.argv[2] ?? process.env.RT_AI_FAKE_PORT ?? 48330);
const state = { replies: [], calls: [], cors: { anthropic: true, gemini: true, openai: false, azure: false, mistral: false, compat: false } };

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const FALLBACK = { text: "fake answer" };

function nextReply() {
  const r = state.replies.length > 1 ? state.replies.shift() : (state.replies[0] ?? FALLBACK);
  return { ...FALLBACK, ...r };
}

function chunks(text) {
  return text.split(/(?<= )/).filter(Boolean);
}

const provider = (path) => path.split("/")[1];
const corsOn = (path) => path.startsWith("/__") || state.cors[provider(path)] === true;

function corsHeaders(path) {
  return corsOn(path)
    ? {
        "access-control-allow-origin": "*",
        "access-control-allow-headers": "*",
        "access-control-allow-methods": "GET, POST, OPTIONS",
      }
    : {};
}

function keyOf(path, h) {
  switch (provider(path)) {
    case "anthropic":
      return h["x-api-key"];
    case "azure":
      return h["api-key"];
    case "gemini":
      return h["x-goog-api-key"];
    case "compat":
      return "optional";
    default:
      return (h.authorization ?? "").replace(/^Bearer /, "");
  }
}

const stopWord = { openai: "tool_calls" };

// Anthropic -------------------------------------------------------------------------------
function anthropic(r, stream) {
  const calls = (r.toolCalls ?? []).map((c, i) => ({ id: c.id ?? `toolu_${i}`, name: c.name, input: c.input ?? {} }));
  const stop = calls.length ? "tool_use" : (r.stop ?? "end_turn");
  if (!stream) {
    return {
      json: {
        id: "msg_fake",
        type: "message",
        role: "assistant",
        content: [...(r.text ? [{ type: "text", text: r.text }] : []), ...calls.map((c) => ({ type: "tool_use", ...c }))],
        stop_reason: stop,
        usage: { input_tokens: 10, output_tokens: 5 },
      },
    };
  }
  const ev = (name, data) => `event: ${name}\ndata: ${JSON.stringify({ type: name, ...data })}\n\n`;
  const out = [ev("message_start", { message: { usage: { input_tokens: 10, output_tokens: 1 } } })];
  let index = 0;
  if (r.text) {
    out.push(ev("content_block_start", { index, content_block: { type: "text", text: "" } }));
    for (const t of chunks(r.text)) out.push(ev("content_block_delta", { index, delta: { type: "text_delta", text: t } }));
    out.push(ev("content_block_stop", { index }));
    index++;
  }
  for (const c of calls) {
    out.push(ev("content_block_start", { index, content_block: { type: "tool_use", id: c.id, name: c.name } }));
    const json = JSON.stringify(c.input);
    out.push(ev("content_block_delta", { index, delta: { type: "input_json_delta", partial_json: json.slice(0, 3) } }));
    out.push(ev("content_block_delta", { index, delta: { type: "input_json_delta", partial_json: json.slice(3) } }));
    out.push(ev("content_block_stop", { index }));
    index++;
  }
  out.push(ev("message_delta", { delta: { stop_reason: stop }, usage: { output_tokens: 5 } }), ev("message_stop", {}));
  return { sse: out };
}

// Chat completions (OpenAI, Azure, Mistral, compatible) -------------------------------------
function chat(r, stream) {
  const calls = (r.toolCalls ?? []).map((c, i) => ({ id: c.id ?? `call_${i}`, name: c.name, input: c.input ?? {} }));
  const finish = calls.length ? "tool_calls" : (r.stop ?? "stop");
  if (!stream) {
    return {
      json: {
        id: "chatcmpl-fake",
        object: "chat.completion",
        choices: [
          {
            index: 0,
            finish_reason: finish,
            message: {
              role: "assistant",
              content: r.text ?? null,
              ...(calls.length
                ? { tool_calls: calls.map((c) => ({ id: c.id, type: "function", function: { name: c.name, arguments: JSON.stringify(c.input) } })) }
                : {}),
            },
          },
        ],
        usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 },
      },
    };
  }
  const chunk = (delta, finish_reason = null) => `data: ${JSON.stringify({ object: "chat.completion.chunk", choices: [{ index: 0, delta, finish_reason }] })}\n\n`;
  const out = [chunk({ role: "assistant" })];
  for (const t of chunks(r.text ?? "")) out.push(chunk({ content: t }));
  calls.forEach((c, index) => {
    const args = JSON.stringify(c.input);
    out.push(chunk({ tool_calls: [{ index, id: c.id, type: "function", function: { name: c.name, arguments: args.slice(0, 3) } }] }));
    out.push(chunk({ tool_calls: [{ index, function: { arguments: args.slice(3) } }] }));
  });
  out.push(chunk({}, finish));
  out.push(`data: ${JSON.stringify({ choices: [], usage: { prompt_tokens: 10, completion_tokens: 5 } })}\n\n`, "data: [DONE]\n\n");
  return { sse: out };
}

// Gemini ------------------------------------------------------------------------------------
function gemini(r, stream) {
  const calls = (r.toolCalls ?? []).map((c) => ({ functionCall: { name: c.name, args: c.input ?? {} } }));
  const body = (parts, finishReason) => ({
    candidates: [{ content: { role: "model", parts }, ...(finishReason ? { finishReason } : {}) }],
    ...(finishReason ? { usageMetadata: { promptTokenCount: 10, candidatesTokenCount: 5 } } : {}),
  });
  const finish = r.stop === "max_tokens" ? "MAX_TOKENS" : "STOP";
  if (!stream) return { json: body([...(r.text ? [{ text: r.text }] : []), ...calls], finish) };
  const out = chunks(r.text ?? "").map((t) => `data: ${JSON.stringify(body([{ text: t }]))}\r\n\r\n`);
  out.push(`data: ${JSON.stringify(body(calls.length ? calls : [{ text: "" }], finish))}\r\n\r\n`);
  return { sse: out };
}

function respond(path, request) {
  const r = nextReply();
  const p = provider(path);
  const stream = p === "gemini" ? path.includes(":streamGenerateContent") : request.stream === true;
  const make = p === "anthropic" ? anthropic : p === "gemini" ? gemini : chat;
  return { reply: r, stream, ...make(r, stream) };
}

async function readBody(req) {
  let text = "";
  for await (const c of req) text += c;
  return text;
}

const server = http.createServer(async (req, res) => {
  const path = decodeURIComponent(req.url.split("?")[0]);
  const send = (status, body, extra = {}) => {
    res.writeHead(status, { "content-type": "application/json", ...corsHeaders(path), ...extra });
    res.end(typeof body === "string" ? body : JSON.stringify(body));
  };
  if (req.method === "OPTIONS") {
    res.writeHead(corsOn(path) ? 204 : 403, corsHeaders(path));
    return res.end();
  }
  if (!["127.0.0.1", "localhost"].includes((req.headers.host ?? "").split(":")[0])) return send(403, { error: "loopback only" });
  if (path === "/__calls") return send(200, state.calls);
  if (path === "/__reset") {
    state.replies = [];
    state.calls = [];
    return send(200, {});
  }
  if (path === "/__script") {
    state.replies = JSON.parse((await readBody(req)) || "{}").replies ?? [];
    return send(200, {});
  }
  if (path === "/__cors") {
    Object.assign(state.cors, JSON.parse((await readBody(req)) || "{}"));
    return send(200, state.cors);
  }
  const known = ["anthropic", "openai", "azure", "mistral", "compat", "gemini"];
  if (req.method !== "POST" || !known.includes(provider(path))) return send(404, { error: { message: "not found" } });

  const text = await readBody(req);
  let body = {};
  try {
    body = JSON.parse(text || "{}");
  } catch {
    return send(400, { error: { message: "body is not JSON" } });
  }
  state.calls.push({ path, headers: { ...req.headers }, body, stream: body.stream === true || path.includes(":stream") });
  if (!keyOf(path, req.headers)) return send(401, { error: { message: "missing API key" } });

  const out = respond(path, body);
  if (out.reply.status && out.reply.status >= 400) {
    return send(out.reply.status, { error: { message: out.reply.error ?? "fake provider error" } });
  }
  if (!out.sse) return send(200, out.json);
  res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-cache", ...corsHeaders(path) });
  for (const piece of out.sse) {
    if (res.destroyed) return;
    res.write(piece);
    await sleep(out.reply.delayMs ?? 5);
  }
  res.end();
});

server.listen(port, "127.0.0.1", () => console.log(`fake AI provider on http://127.0.0.1:${port}`));
