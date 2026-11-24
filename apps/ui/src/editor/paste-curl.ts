// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// A pasted cURL command or share link in the URL field (wave 17, strand C). A paste that the
// core can read as one of them gets a hint, "This looks like a cURL command. Turn it into a
// request"; Enter takes it, Escape leaves the text. The cURL command is read by the importer
// of the core (the same one `AiTask` "request-from-snippet" uses for a cURL command, without
// a model); a link is opened as an unsaved draft. No request is made, nothing is sent.

import type { AiAnswer, RequestSpec } from "../commands/types";
import { classifyPaste, type PasteKind } from "../share/detect";
import type { AppState } from "../state/app.svelte";

/** What a paste offers to become. */
export interface PasteOffer {
  kind: PasteKind;
  /** The text as pasted (the field itself drops line breaks). */
  text: string;
}

/** The offer for pasted text, or `null` when it is nothing to offer for. */
export function offerFor(text: string): PasteOffer | null {
  const kind = classifyPaste(text);
  return kind ? { kind, text } : null;
}

/** The sentence of the hint. */
export function hintText(offer: PasteOffer, replaces: boolean): string {
  return offer.kind === "curl"
    ? replaces
      ? "This looks like a cURL command. Replace this request with it"
      : "This looks like a cURL command. Turn it into a request"
    : "This looks like a shared request. Open it as a draft";
}

/** The request a cURL command describes, with the importer's note on what it left out. */
export async function readCurl(app: AppState, text: string): Promise<AiAnswer & { request: RequestSpec }> {
  const answer = await app.backend.ai.ask(
    { task: "request-from-snippet", snippet: text.trim(), folder: "" },
    { environment: app.environment, vars: [] },
  );
  if (!answer.request) throw new Error("The cURL command holds no request.");
  return answer as AiAnswer & { request: RequestSpec };
}

/** Replaces the request at `path` with the cURL command (name and order stay); unsaved.
 *  Returns the importer's note (what it left out). */
export async function applyCurl(app: AppState, path: string, text: string): Promise<string> {
  const answer = await readCurl(app, text);
  const current = app.drafts[path] ?? (await app.backend.getRequest(path));
  await app.applyExternal(path, { ...answer.request, name: current.name, seq: current.seq });
  return answer.text;
}

/** A new request from the cURL command in `folder` of the active collection (unsaved). */
export async function askCurl(app: AppState, text: string, folder = ""): Promise<string> {
  const answer = await readCurl(app, text);
  await app.create(folder, answer.request.name || "New request");
  const path = app.active;
  if (!path) throw new Error("The request could not be opened.");
  const created = await app.backend.getRequest(path);
  await app.applyExternal(path, { ...answer.request, name: created.name, seq: created.seq });
  return answer.text;
}

/** Takes the offer for the request at `path`: a cURL command replaces it, a link opens as
 *  a new draft. Returns a note to show, if any. */
export async function takeOffer(app: AppState, path: string, offer: PasteOffer): Promise<string | null> {
  if (offer.kind === "curl") return applyCurl(app, path, offer.text);
  await app.openShared(offer.text);
  return null;
}
