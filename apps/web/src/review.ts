// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ReviewApi` of the web app (wave 12, strand E): pull and merge requests of the collection's
// Git host, over rt-wasm's review functions (crates/rt-wasm/src/review_js.rs).
//
// - The host comes from `origin` (`Workspace.reviewDetect`); self-hosted hosts are kept per
//   browser in `localStorage` (`requesttailor.review.hosts`), never in the collection.
// - The token lives in the encrypted browser secret store (collection `review:tokens`,
//   environment = host, variable `token`: `rt_app::review::token_slot`); the UI can set and
//   delete it and ask whether one exists, never read it back.
// - API calls go out with `fetch`, or through the companion when it is paired. Opening a pull
//   request fetches its head with isomorphic-git (./git/review.ts) and needs the companion,
//   like every Git request of the web app; so does the device-code login.

import {
  BackendError,
  type CiStatus,
  type DeviceCodePrompt,
  type NewReviewComment,
  type PullRequest,
  type ReviewApi,
  type ReviewComment,
  type ReviewHost,
  type ReviewRepo,
  type ReviewSession,
  type ReviewSubmission,
  type SecretsApi,
} from "@requesttailor/ui";

import { directoryFs } from "./git/fs";
import { companionHttp, NO_COMPANION_MESSAGE } from "./git/http";
import type { Repo } from "./git/repo";
import { toBackendError, type CompanionCredentials, type CoreWorkspace } from "./wasm";
import type { Wave12Host } from "./wave12";

/** `rt_app::review::{TOKEN_COLLECTION, TOKEN_VARIABLE}`. */
export const TOKEN_COLLECTION = "review:tokens";
export const TOKEN_VARIABLE = "token";
export const HOSTS_STORAGE_KEY = "requesttailor.review.hosts";

/** What the core's `device::start` returns (`rt_app::review::device::DeviceLogin`). */
export interface DeviceLogin {
  prompt: DeviceCodePrompt;
  device_code: string;
  interval: number;
}

type DevicePoll = { state: "pending" } | { state: "slow-down"; interval: number } | { state: "token"; token: string };

/** The review part of rt-wasm's `Workspace`. */
export interface CoreReviewWorkspace extends CoreWorkspace {
  reviewDetect(remoteUrl: string, hosts: ReviewHost[]): ReviewRepo | null;
  reviewNormalizeHosts(hosts: ReviewHost[]): ReviewHost[];
  reviewCall(
    repo: ReviewRepo,
    token: string,
    op: "list" | "get" | "comments" | "comment" | "submit" | "ci" | "create",
    number: number,
    payload: unknown,
    companion: CompanionCredentials | null,
  ): Promise<unknown>;
  reviewDeviceClientId(repo: ReviewRepo): string | null;
  reviewDeviceStart(repo: ReviewRepo, clientId: string, companion: CompanionCredentials | null): Promise<DeviceLogin>;
  reviewDevicePoll(repo: ReviewRepo, clientId: string, login: DeviceLogin, companion: CompanionCredentials | null): Promise<DevicePoll>;
}

async function guard<T>(f: () => Promise<T>): Promise<T> {
  try {
    return await f();
  } catch (e) {
    throw toBackendError(e);
  }
}

function browserStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

function parseHosts(raw: string | null): ReviewHost[] {
  if (!raw) return [];
  try {
    const v: unknown = JSON.parse(raw);
    if (!Array.isArray(v)) return [];
    return v.filter(
      (h): h is ReviewHost =>
        typeof h === "object" &&
        h !== null &&
        typeof (h as ReviewHost).host === "string" &&
        typeof (h as ReviewHost).api === "string" &&
        ((h as ReviewHost).kind === "github" || (h as ReviewHost).kind === "gitlab"),
    );
  } catch {
    return [];
  }
}

const sleep = (seconds: number) => new Promise<void>((resolve) => setTimeout(resolve, seconds * 1000));

export function webReview(w: Wave12Host, storage: () => Storage | null = browserStorage): ReviewApi {
  let tokens: Promise<SecretsApi> | null = null;
  const tokenStore = () => {
    tokens ??= w.host.deps.openSecrets(TOKEN_COLLECTION).catch((e: unknown) => {
      tokens = null;
      throw e;
    });
    return tokens;
  };
  const workspace = () => w.host.session().ws as CoreReviewWorkspace;
  const gitRepo = (): Repo => ({ fs: directoryFs(w.host.session().handle), dir: "/", cache: {} });
  const logins = new Map<string, DeviceLogin>();

  const readHosts = (): ReviewHost[] => {
    try {
      return parseHosts(storage()?.getItem(HOSTS_STORAGE_KEY) ?? null);
    } catch {
      return [];
    }
  };

  const detect = async (): Promise<ReviewRepo | null> => {
    const { originUrl } = await import("./git/review");
    const url = await originUrl(gitRepo());
    return url === null ? null : workspace().reviewDetect(url, readHosts());
  };

  const requireRepo = async (): Promise<ReviewRepo> => {
    const repo = await detect();
    if (!repo) {
      throw new BackendError(
        "not-found",
        "the origin of this collection is not on a Git host the app knows; add the host under Git hosts",
      );
    }
    return repo;
  };

  const readToken = async (host: string): Promise<string | null> => {
    const t = await (await tokenStore()).get(host, TOKEN_VARIABLE);
    return t && t.trim() !== "" ? t : null;
  };

  const requireToken = async (host: string): Promise<string> => {
    const t = await readToken(host);
    if (t === null) throw new BackendError("no-token", `no token for ${host}`);
    return t;
  };

  /** One call of the host's API with the stored token. */
  const call = async <T>(op: Parameters<CoreReviewWorkspace["reviewCall"]>[2], number: number, payload: unknown = null): Promise<T> => {
    const repo = await requireRepo();
    const token = await requireToken(repo.host);
    const companion = await w.host.companionCredentials();
    return (await workspace().reviewCall(repo, token, op, number, payload, companion)) as T;
  };

  return {
    repo: () => guard(detect),
    hosts: async () => readHosts(),
    setHosts: (hosts) =>
      guard(async () => {
        const normalized = workspace().reviewNormalizeHosts(hosts);
        const s = storage();
        if (!s) throw new BackendError("shell", "This browser does not allow saving settings.");
        s.setItem(HOSTS_STORAGE_KEY, JSON.stringify(normalized));
      }),
    hasToken: (host) => guard(async () => (await readToken(host)) !== null),
    setToken: (host, token) =>
      guard(async () => {
        const store = await tokenStore();
        if (token.trim() === "") await store.delete(host, TOKEN_VARIABLE);
        else await store.set(host, TOKEN_VARIABLE, token.trim());
      }),
    deleteToken: (host) => guard(async () => (await tokenStore()).delete(host, TOKEN_VARIABLE)),

    startLogin: (host) =>
      guard(async () => {
        const repo = await requireRepo();
        if (repo.host.toLowerCase() !== host.toLowerCase()) {
          throw new BackendError("not-found", `${host} is not this collection's Git host`);
        }
        const clientId = workspace().reviewDeviceClientId(repo);
        if (!clientId) {
          throw new BackendError("unsupported", "Signing in with a code is not available for this host; use a personal access token.");
        }
        const companion = await w.host.companionCredentials();
        if (!companion) throw new BackendError("unsupported", `${NO_COMPANION_MESSAGE} Signing in with a code needs it.`);
        const login = await workspace().reviewDeviceStart(repo, clientId, companion);
        logins.set(host, login);
        return login.prompt;
      }),
    finishLogin: (host) =>
      guard(async () => {
        const repo = await requireRepo();
        const login = logins.get(host);
        const clientId = workspace().reviewDeviceClientId(repo);
        if (!login || !clientId) throw new BackendError("shell", "Start the login first.");
        logins.delete(host);
        const companion = await w.host.companionCredentials();
        let interval = login.interval;
        let waited = 0;
        for (;;) {
          if (waited > login.prompt.expires_in) throw new BackendError("auth", "the code expired; start the login again");
          await sleep(interval);
          waited += interval;
          const polled = await workspace().reviewDevicePoll(repo, clientId, login, companion);
          if (polled.state === "token") {
            await (await tokenStore()).set(host, TOKEN_VARIABLE, polled.token);
            return;
          }
          if (polled.state === "slow-down") interval = polled.interval;
        }
      }),

    list: () => guard(() => call<PullRequest[]>("list", 0)),

    open: (number) =>
      guard(async (): Promise<ReviewSession> => {
        const repo = await requireRepo();
        const token = await requireToken(repo.host);
        const companion = await w.host.companionCredentials();
        if (!companion) throw new BackendError("unsupported", `${NO_COMPANION_MESSAGE} Opening a pull request fetches its commits.`);
        const pull = (await workspace().reviewCall(repo, token, "get", number, null, companion)) as PullRequest;
        const { fetchReview } = await import("./git/review");
        const fetched = await fetchReview(gitRepo(), companionHttp(companion), {
          kind: repo.kind,
          number,
          remoteHeadRef: repo.kind === "github" ? `refs/pull/${number}/head` : `refs/merge-requests/${number}/head`,
          target: pull.target,
          host: repo.host,
          token,
        });
        return { pull, head_ref: fetched.head_ref, merge_base: fetched.merge_base };
      }),

    comments: (number) => guard(() => call<ReviewComment[]>("comments", number)),
    comment: (number, comment: NewReviewComment) => guard(() => call<ReviewComment>("comment", number, comment)),
    submit: (number, review: ReviewSubmission) => guard(async () => void (await call<null>("submit", number, review))),
    ci: (number) => guard(() => call<CiStatus>("ci", number)),
    create: (pull) => guard(() => call<PullRequest>("create", 0, pull)),
  };
}
