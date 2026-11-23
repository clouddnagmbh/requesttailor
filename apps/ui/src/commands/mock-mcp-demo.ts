// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The MCP demo server in the mock backend (wave 14), for UI tests: the same tools, resources
// and prompts as rt_app::mcp_demo, with the same fake weather (FNV-1a of the city), a to-do
// list per connection and countdown notifications. Also a stand-in for `rt mcp` (the five
// agent tools with canned answers). The real server and its protocol are tested in Rust.

import type {
  McpCallResult,
  McpContent,
  McpNotification,
  McpPrompt,
  McpPromptResult,
  McpResource,
  McpResourceContents,
  McpTool,
} from "./types";

const obj = (properties: Record<string, unknown>, required: string[] = []) => ({ type: "object", properties, required });
const todoSchema = obj({ id: { type: "integer" }, title: { type: "string" }, done: { type: "boolean" } }, ["id", "title", "done"]);
const ro = { readOnlyHint: true, openWorldHint: false };

function tool(name: string, title: string, description: string, input: unknown, output: unknown = null, annotations: unknown = ro): McpTool {
  return { name, title, description, input_schema: input, output_schema: output, annotations };
}

export const DEMO_TOOLS: McpTool[] = [
  tool("echo", "Echo", "Returns the text it gets. The simplest possible call.", obj({ text: { type: "string", description: "What to echo back" } }, ["text"])),
  tool(
    "add",
    "Add two numbers",
    "Adds a and b. Answers with structured output that matches its outputSchema.",
    obj({ a: { type: "number", description: "First number" }, b: { type: "number", description: "Second number" } }, ["a", "b"]),
    obj({ sum: { type: "number" } }, ["sum"]),
  ),
  tool(
    "fake_weather",
    "Fake weather",
    "Made-up weather for any city. Deterministic: the same city always gets the same weather.",
    obj(
      {
        city: { type: "string", description: "Any city name, e.g. Vienna" },
        units: { type: "string", enum: ["metric", "imperial"], default: "metric", description: "metric: °C and km/h; imperial: °F and mph" },
      },
      ["city"],
    ),
    obj({ city: { type: "string" }, temperature: { type: "number" }, units: { type: "string" }, conditions: { type: "string" }, humidity: { type: "integer" }, wind: { type: "number" } }),
    { readOnlyHint: true, idempotentHint: true, openWorldHint: false },
  ),
  tool("todo_add", "Add a to-do", "Adds a to-do to this session's list (kept in memory until you disconnect).", obj({ title: { type: "string", description: "What to do" } }, ["title"]), todoSchema, {
    readOnlyHint: false,
    destructiveHint: false,
    openWorldHint: false,
  }),
  tool("todo_list", "List to-dos", "Lists this session's to-dos.", obj({ open_only: { type: "boolean", description: "Only the ones not done yet", default: false } }), obj({ items: { type: "array", items: todoSchema }, open: { type: "integer" } })),
  tool("todo_done", "Mark a to-do done", "Marks the to-do with this id as done. Unknown ids are a tool error.", obj({ id: { type: "integer", minimum: 1, description: "From todo_add or todo_list" } }, ["id"]), todoSchema, {
    readOnlyHint: false,
    destructiveHint: false,
    idempotentHint: true,
    openWorldHint: false,
  }),
  tool("uuid", "UUID", "Returns a UUID in version 4 format. Deterministic per session (the first one is always the same), so saved tests stay stable.", obj({}), obj({ uuid: { type: "string" } })),
  tool("image", "Image", "Returns a small PNG image, to show image results.", obj({ color: { type: "string", enum: ["blue", "green", "orange", "purple"], default: "blue" } })),
  tool("fail", "Fail", "Always fails with isError: true, to show how tool errors look.", obj({ message: { type: "string", description: "The error message" } })),
  tool("countdown", "Countdown", "Counts down and sends a progress and a log notification per step before the result.", obj({ from: { type: "integer", minimum: 1, maximum: 10, default: 3, description: "Where to start" } }), obj({ steps: { type: "integer" } })),
];

export const DEMO_RESOURCES: McpResource[] = [
  { uri: "demo://readme", name: "README", title: "About the demo server", description: "What the demo server offers, in Markdown.", mime_type: "text/markdown", size: null },
  { uri: "demo://cities.json", name: "cities.json", title: "Cities", description: "Cities to try with fake_weather, with today's fake weather.", mime_type: "application/json", size: null },
  { uri: "demo://todos", name: "todos", title: "To-dos of this session", description: "The to-do list of the todo_* tools, as JSON.", mime_type: "application/json", size: null },
];

export const DEMO_PROMPTS: McpPrompt[] = [
  {
    name: "summarize",
    title: "Summarize a text",
    description: "Asks the model to summarize a text, as bullets or one sentence.",
    arguments: [
      { name: "text", title: null, description: "The text to summarize", required: true },
      { name: "style", title: null, description: "bullets (default) or sentence", required: false },
    ],
  },
  { name: "weather_report", title: "Weather report", description: "A short weather report for a city, with the fake weather attached.", arguments: [{ name: "city", title: null, description: "Any city name", required: true }] },
];

const CONDITIONS = ["sunny", "partly cloudy", "cloudy", "light rain", "thunderstorms", "fog"];
const MASK = (1n << 64n) - 1n;

function fnv(text: string): bigint {
  let h = 0xcbf29ce484222325n;
  for (const b of new TextEncoder().encode(text)) {
    h ^= BigInt(b);
    h = (h * 0x100000001b3n) & MASK;
  }
  return h;
}

/** As rt_app::mcp_demo: the same city always gets the same weather. */
export function demoWeather(city: string, units: "metric" | "imperial" = "metric") {
  const name = city.trim();
  const h = fnv(name.toLowerCase());
  const celsius = Number(h % 36n) - 5;
  const conditions = celsius <= 1 && (h >> 40n) % 2n === 0n ? "snow" : CONDITIONS[Number((h >> 8n) % 6n)]!;
  const humidity = 30 + Number((h >> 16n) % 61n);
  const kmh = Number((h >> 24n) % 41n);
  const imperial = units === "imperial";
  return {
    city: name.charAt(0).toUpperCase() + name.slice(1),
    temperature: imperial ? Math.round((celsius * 9) / 5 + 32) : celsius,
    units,
    conditions,
    humidity,
    wind: imperial ? Math.round(kmh * 0.621371) : kmh,
  };
}

const PIXEL = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

const text = (t: string): McpContent => ({ type: "text", text: t });
const done = (content: McpContent[], structured: unknown = null, isError = false, notifications: McpNotification[] = []): McpCallResult => ({
  content,
  structured,
  is_error: isError,
  time_ms: 1,
  ...(notifications.length > 0 ? { notifications } : {}),
});
const structuredResult = (value: unknown, summary?: string) => done([...(summary ? [text(summary)] : []), text(JSON.stringify(value))], value);
const toolError = (message: string) => done([text(message)], null, true);

/** One connection's state: the to-do list and the UUID counter. */
export class DemoSession {
  private todos: { id: number; title: string; done: boolean }[] = [];
  private uuids = 0;

  call(name: string, args: Record<string, unknown>): McpCallResult | null {
    switch (name) {
      case "echo":
        return typeof args.text === "string" ? done([text(args.text)]) : toolError('echo needs text: {"text": "hello"}');
      case "add":
        return typeof args.a === "number" && typeof args.b === "number" ? structuredResult({ sum: args.a + args.b }) : toolError('add needs two numbers: {"a": 2, "b": 3}');
      case "fake_weather": {
        const city = typeof args.city === "string" ? args.city.trim() : "";
        if (city === "") return toolError('fake_weather needs a city: {"city": "Vienna"}');
        const units = args.units === "imperial" ? "imperial" : "metric";
        const w = demoWeather(city, units);
        const [t, s] = units === "metric" ? ["°C", "km/h"] : ["°F", "mph"];
        return structuredResult(w, `${w.city}: ${w.temperature} ${t}, ${w.conditions}, humidity ${w.humidity} %, wind ${w.wind} ${s} (fake data)`);
      }
      case "todo_add": {
        const title = typeof args.title === "string" ? args.title.trim() : "";
        if (title === "") return toolError('todo_add needs a title: {"title": "Water the plants"}');
        const todo = { id: this.todos.length + 1, title, done: false };
        this.todos.push(todo);
        return structuredResult({ ...todo }, `Added to-do ${todo.id}: ${title}`);
      }
      case "todo_list": {
        const items = this.todos.filter((t) => !args.open_only || !t.done).map((t) => ({ ...t }));
        return structuredResult({ items, open: this.todos.filter((t) => !t.done).length });
      }
      case "todo_done": {
        const todo = this.todos.find((t) => t.id === args.id);
        if (!todo) return toolError(`There is no to-do ${String(args.id)}. Call todo_list to see the ids.`);
        todo.done = true;
        return structuredResult({ ...todo }, `Done: ${todo.title}`);
      }
      case "uuid":
        this.uuids += 1;
        return structuredResult({ uuid: `5f0c7d2e-1a3b-4c5d-8e9f-${String(this.uuids).padStart(12, "0")}` });
      case "image":
        return done([{ type: "image", mime_type: "image/png", data: PIXEL }, text("A 64×64 PNG drawn by the demo server.")]);
      case "fail":
        return toolError(typeof args.message === "string" && args.message.trim() !== "" ? args.message : "This tool always fails.");
      case "countdown": {
        const from = typeof args.from === "number" ? args.from : 3;
        if (!Number.isInteger(from) || from < 1 || from > 10) return toolError("from must be a whole number from 1 to 10");
        const notes: McpNotification[] = [];
        for (let step = 1; step <= from; step++) {
          const left = from - step + 1;
          notes.push({ method: "notifications/progress", params: { progressToken: 1, progress: step, total: from, message: `step ${step} of ${from}` } });
          notes.push({ method: "notifications/message", params: { level: "info", logger: "demo", data: `T minus ${left}` } });
        }
        return done([text("Liftoff!"), text(JSON.stringify({ steps: from }))], { steps: from }, false, notes);
      }
      default:
        return null;
    }
  }

  read(uri: string): McpResourceContents[] | null {
    switch (uri) {
      case "demo://readme":
        return [{ uri, mime_type: "text/markdown", text: "# RequestTailor demo server\n\nA small MCP server with fake data, built into RequestTailor.\n", blob: null }];
      case "demo://cities.json":
        return [{ uri, mime_type: "application/json", text: JSON.stringify({ cities: ["Vienna", "Berlin", "Lisbon", "Oslo", "Tokyo", "Nairobi"].map((c) => demoWeather(c)) }, null, 2), blob: null }];
      case "demo://todos":
        return [{ uri, mime_type: "application/json", text: JSON.stringify({ items: this.todos }, null, 2), blob: null }];
      default:
        return null;
    }
  }

  prompt(name: string, args: Record<string, string>): McpPromptResult | null {
    if (name === "summarize") {
      const how = args.style === "sentence" ? "in one sentence" : "as three short bullet points";
      return { description: "Summarize a text", messages: [{ role: "user", content: text(`Summarize the following text ${how}:\n\n${args.text ?? ""}`) }] };
    }
    if (name === "weather_report") {
      const w = demoWeather(args.city ?? "");
      return {
        description: `Weather report for ${w.city}`,
        messages: [
          { role: "user", content: text(`Write a friendly two-sentence weather report for ${w.city} from this data:`) },
          { role: "user", content: { type: "resource", uri: `demo://weather/${w.city.toLowerCase()}`, text: JSON.stringify(w) } },
        ],
      };
    }
    return null;
  }
}

/** `rt mcp` in the mock: its five tools with canned answers. */
export const SELF_TOOLS: McpTool[] = [
  tool("list_requests", "List requests", "Path, name, method and effect (read or write) of every request.", obj({ folder: { type: "string" } })),
  tool("get_request", "Get a request", "One request as in its file; credentials are masked.", obj({ path: { type: "string" } }, ["path"])),
  tool("list_environments", "List environments", "The environments the agent may use, with access and production flag.", obj({})),
  tool("send_request", "Send a request", "Sends one request and returns the report with secrets masked.", obj({ path: { type: "string" }, environment: { type: "string" } }, ["path"]), null, { readOnlyHint: false, openWorldHint: true }),
  tool("run_folder", "Run a folder", "Runs a folder or the whole collection like rt run.", obj({ folder: { type: "string" }, environment: { type: "string" } }), null, { readOnlyHint: false, openWorldHint: true }),
];

export function selfCall(name: string): McpCallResult | null {
  switch (name) {
    case "list_requests":
      return structuredResult({ requests: [{ path: "users/get-user.yml", name: "Get user", method: "GET", effect: "read" }] });
    case "list_environments":
      return structuredResult({ environments: [{ name: "Local", access: "read", production: false, secrets: ["token"] }] });
    case "get_request":
    case "send_request":
    case "run_folder":
      return structuredResult({ ok: true });
    default:
      return null;
  }
}
