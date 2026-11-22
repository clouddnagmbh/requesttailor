<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

Draft notes for the release with Git for APIs. Everything here is free and open source; the
app talks only to your own Git host, never to our servers.

## Git for APIs

- **Semantic diff.** A commit, your saved changes or two branches, shown per request field
  (URL, method, headers, parameters, body, auth, scripts, assertions, settings, folders,
  environments) instead of YAML lines; renames show as moves, and the line diff is one click
  away. Secret values never appear. `rt diff` prints the same on the command line, also as
  JSON. 1000 requests with 100 changes are compared in under 200 ms (a new performance budget).
- **Semantic merge.** Pull merges collection files field by field: two people who change
  different fields of one request no longer get a conflict. When both change the same field,
  the app lets you pick per field. For the Git command line, `rt merge-driver` does the same
  in `git merge`, `pull`, `rebase` and `cherry-pick`:
  `git config merge.requesttailor.driver "rt merge-driver %O %A %B %P"` and
  `*.yml merge=requesttailor` in `.gitattributes`.
- **Compare and bisect.** Run the requests of any commit or branch without checking it out,
  with today's environments and secrets, and compare two of them: status, headers, JSON body
  per JSON Pointer, tests. Bisect finds the commit that broke a test, in the app with progress
  or with `rt bisect` (50 commits in at most 7 runs).
- **Request history.** Every version of a request across commits and renames, blame per field,
  the test results recorded per commit (on your machine, never in the repository), and an old
  version to look at, send or restore.
- **Pull request review.** Open GitHub pull requests and GitLab merge requests (also GitHub
  Enterprise and self-hosted GitLab) with your personal access token, review the semantic
  diff, run the changed requests against your environment, comment on lines, approve or ask
  for changes, and see the CI status. The token is kept in the OS keychain or the encrypted
  browser storage.
- **API contract.** Point the collection at its OpenAPI spec (or keep `openapi.yaml` next to
  it): the app finds requests without an operation, wrong parameters and bodies that do not
  match, and marks breaking changes between two revisions in the diff and in reviews.
  `rt changelog v1..v2` writes the API changelog in Markdown.
- **Secret rescue.** Scans the whole history of your branches for credentials that slipped
  into a commit, rewrites those commits with a `{{variable}}` in place of the value after you
  confirm, keeps a backup of every branch, and moves the value into a secret variable. It never
  force-pushes: it shows the command and reminds you to rotate the secret.

## Not in this release

- Rebase and cherry-pick in the app (the Git command line uses `rt merge-driver` for them).
- Signing in to GitHub and GitLab with a device code; personal access tokens work today.
- Gitea and Bitbucket reviews, JUnit results of pipeline runs in reviews.
- Recording `rt run` and `rt bisect` results per commit; runs in the app are recorded.
- Checking responses against the spec's response schemas.
