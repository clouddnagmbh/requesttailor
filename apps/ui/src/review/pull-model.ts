// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Pure helpers of the pull-request view that read a collection's field diff (wave 12, strand E):
// which requests a PR changed, which field a comment's line belongs to. The generic review
// helpers live in `@tailor-kit/git-ui`.

import type { CollectionDiff, FieldPath, ReviewComment } from "../commands/types";

/** Paths (at the head) of the requests a PR added, changed or moved: the ones worth running. */
export function changedRequestPaths(diff: CollectionDiff): string[] {
  const paths: string[] = [];
  for (const c of diff.changes) {
    if (c.target.kind !== "request" || c.change.kind === "removed") continue;
    if (!paths.includes(c.target.path)) paths.push(c.target.path);
  }
  return paths;
}

/** Whether a repository-relative `path` of a comment is `file` of the collection (the
 *  collection may sit in a sub-folder of the repository). */
function sameFile(path: string, file: string): boolean {
  return path === file || path.endsWith(`/${file}`);
}

/** The field of the diff at `line` of `path`, if the diff knows one. */
export function fieldForLine(diff: CollectionDiff, path: string, line: number): FieldPath | undefined {
  for (const c of diff.changes) {
    if (!sameFile(path, c.file)) continue;
    const hit = c.fields.find((f) => f.line === line);
    if (hit) return hit.path;
  }
  return undefined;
}

/** `comments` with `field` set where a line comment sits on a changed field. */
export function withFields(comments: ReviewComment[], diff: CollectionDiff | null): ReviewComment[] {
  if (!diff) return comments;
  return comments.map((c) => {
    if (c.path === null || c.line === null || c.outdated || c.field) return c;
    const field = fieldForLine(diff, c.path, c.line);
    return field ? { ...c, field } : c;
  });
}
