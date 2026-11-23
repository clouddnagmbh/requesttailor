# AI test data

`fake-provider.mjs` is a local fake of the AI providers' wire formats (Anthropic Messages,
OpenAI, Azure OpenAI, Mistral, OpenAI-compatible, Gemini), with SSE streaming and tool use.
Tests never call a real provider.

    node testdata/ai/fake-provider.mjs [port]     # default RT_AI_FAKE_PORT or 48330

Control endpoints: `POST /__script` (next answers), `GET /__calls`, `POST /__reset`,
`POST /__cors` (CORS per provider prefix).

`live-check.mjs` compares the real wire formats with what the code expects. It runs only with
`RT_AI_LIVE=1` and one `RT_AI_LIVE_<PROVIDER>_KEY` per provider, never in CI.
