// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Fetching a pull request's head for review in the web app (isomorphic-git; wave 12, strand E),
// the same as `rt-git/src/review.rs`: the head (`refs/pull/<n>/head`,
// `refs/merge-requests/<n>/head`) lands in `refs/requesttailor/review/<kind>/<n>` and the
// target branch's tip in `…/<n>-base`, without a checkout. The network is the companion's
// (`companionHttp`), like every Git request of the web app; the user's token is the HTTPS
// password for the review host only.

import "./buffer-global";

import git from "isomorphic-git";
import type { HttpClient as IsoHttpClient } from "isomorphic-git";

import { BackendError, type ReviewKind } from "@requesttailor/ui";

import { gitError, type Repo } from "./repo";
import { hostOf, type HttpClient } from "./http";
import { mergeBase, revParse } from "./revisions";

/** `rt_app::review::review_ref`. */
export const reviewRef = (kind: ReviewKind, number: number): string => `refs/requesttailor/review/${kind}/${number}`;
/** `rt_app::review::review_base_ref`. */
export const reviewBaseRef = (kind: ReviewKind, number: number): string => `refs/requesttailor/review/${kind}/${number}-base`;

export interface ReviewFetch {
  kind: ReviewKind;
  number: number;
  /** The ref on the remote that holds the head (`ReviewProvider.head_ref`). */
  remoteHeadRef: string;
  /** The PR's target branch. */
  target: string;
  /** The review host (as in `ReviewRepo.host`): the only host the token may go to. */
  host: string;
  token: string;
}

export interface FetchedReview {
  head_ref: string;
  head: string;
  merge_base: string;
}

/** The URL of `origin`, or `null` (no repository, no remote). */
export async function originUrl(repo: Repo): Promise<string | null> {
  try {
    const url = await git.getConfig({ fs: repo.fs, dir: repo.dir, path: "remote.origin.url" });
    return typeof url === "string" && url !== "" ? url : null;
  } catch {
    return null;
  }
}

async function remoteUrl(repo: Repo): Promise<string> {
  const url = await originUrl(repo);
  if (url === null) throw new BackendError("git", "This collection has no remote named origin.");
  return url;
}

export async function fetchReview(repo: Repo, http: HttpClient, spec: ReviewFetch): Promise<FetchedReview> {
  const url = await remoteUrl(repo);
  if (!/^https?:\/\//i.test(url)) {
    throw new BackendError("git", "The web app fetches over HTTPS; this collection's origin is not an https:// URL.");
  }
  const host = hostOf(url);
  const username = spec.kind === "github" ? "x-access-token" : "oauth2";
  // The token only goes to the review host (a remote on another host gets none).
  const sameHost = host.toLowerCase() === spec.host.toLowerCase();
  const fetchOne = async (remoteRef: string, localRef: string): Promise<void> => {
    let tried = false;
    try {
      const result = await git.fetch({
        fs: repo.fs,
        dir: repo.dir,
        cache: repo.cache,
        http: http as unknown as IsoHttpClient,
        remote: "origin",
        // The branch name is only for isomorphic-git's bookkeeping; what we keep is the
        // fetched head, written to our own ref below.
        ref: remoteRef,
        remoteRef,
        singleBranch: true,
        tags: false,
        onAuth: () => {
          if (tried || !sameHost) return { cancel: true };
          tried = true;
          return { username, password: spec.token };
        },
        onAuthFailure: () => ({ cancel: true }),
      });
      if (!result.fetchHead) throw new BackendError("git", `The remote has no ${remoteRef}.`);
      await git.writeRef({ fs: repo.fs, dir: repo.dir, ref: localRef, value: result.fetchHead, force: true });
    } catch (e) {
      throw gitError(e, host);
    }
  };
  const headRef = reviewRef(spec.kind, spec.number);
  const baseRef = reviewBaseRef(spec.kind, spec.number);
  await fetchOne(spec.remoteHeadRef, headRef);
  await fetchOne(`refs/heads/${spec.target.replace(/^refs\/heads\//, "")}`, baseRef);
  const head = await revParse(repo, headRef);
  const merge_base = await mergeBase(repo, headRef, baseRef);
  if (!merge_base) throw new BackendError("git", "The pull request shares no history with its target branch.");
  return { head_ref: headRef, head, merge_base };
}
