<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- One pull request, reviewed locally (wave 12, strand E): its head is fetched without a
     checkout; the semantic diff is merge base → head (diff/SemanticDiff.svelte, comments at
     fields); the API's breaking changes (contract/ContractNotes.svelte); the changed requests
     run at the head against the reviewer's own environment; comments, a verdict, CI status. -->
<script lang="ts">
  import { onMount } from "svelte";

  import ContractNotes from "../contract/ContractNotes.svelte";
  import ExtensionDiffNotes from "../shell/ExtensionDiffNotes.svelte";
  import type {
    ChangeTarget,
    CiStatus,
    CollectionDiff,
    FieldPath,
    ReviewComment,
    ReviewRepo,
    ReviewSession,
    ReviewVerdict,
    RevisionRun,
  } from "../commands/types";
  import { fieldLabel } from "../diff/format";
  import SemanticDiff from "../diff/SemanticDiff.svelte";
  import { getApp } from "../state/app.svelte";
  import { changedRequestPaths, CI_LABEL, groupComments, relativeTime, requestWord, stateLabel, withFields } from "./model";

  let {
    number,
    repo,
    onback,
    onauth,
  }: {
    number: number;
    repo: ReviewRepo;
    onback: () => void;
    /** The host rejected the token (or there is none): the list asks for one again. */
    onauth: (message: string | null) => void;
  } = $props();

  const app = getApp();

  let session = $state<ReviewSession | null>(null);
  let diff = $state<CollectionDiff | null>(null);
  let comments = $state<ReviewComment[]>([]);
  let ci = $state<CiStatus | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(true);

  let running = $state(false);
  let run = $state<RevisionRun | null>(null);
  let runError = $state<string | null>(null);

  let composer = $state<{ path: string; line: number; label: string } | null>(null);
  let draft = $state("");
  let verdictBody = $state("");
  let sending = $state(false);
  let sent = $state<string | null>(null);
  let actionError = $state<string | null>(null);

  const word = $derived(requestWord(repo));
  const changed = $derived(diff ? changedRequestPaths(diff) : []);
  const groups = $derived(groupComments(comments));
  const VERDICT_DONE: Record<ReviewVerdict, string> = {
    approve: "Approved.",
    "request-changes": "Changes requested.",
    comment: "Comment posted.",
  };

  function message(e: unknown): string {
    return e instanceof Error ? e.message : String(e);
  }

  /** `true` if it was a token problem (the list takes over). */
  function authProblem(e: unknown): boolean {
    const kind = (e as { kind?: string }).kind;
    if (kind === "no-token" || kind === "auth") {
      onauth(kind === "auth" ? message(e) : null);
      return true;
    }
    return false;
  }

  async function load() {
    loading = true;
    error = null;
    try {
      const opened = await app.backend.review.open(number);
      session = opened;
      const [d, c, status] = await Promise.all([
        app.backend.diffs.between(opened.merge_base, opened.head_ref),
        app.backend.review.comments(number),
        // The CI status is a hint: a host without checks must not block the review.
        app.backend.review.ci(number).catch(() => null),
      ]);
      diff = d;
      comments = withFields(c, d);
      ci = status;
    } catch (e) {
      if (!authProblem(e)) error = message(e);
    } finally {
      loading = false;
    }
  }

  onMount(() => void load());

  async function runChanged() {
    if (!session || running) return;
    running = true;
    runError = null;
    run = null;
    try {
      run = await app.backend.compare.run({ rev: session.head_ref, paths: changed }, { environment: app.environment, vars: [] });
    } catch (e) {
      runError = message(e);
    } finally {
      running = false;
    }
  }

  function startComment(_target: ChangeTarget, field: FieldPath, file: string, line: number) {
    composer = { path: file, line, label: fieldLabel(field) };
    draft = "";
    actionError = null;
  }

  async function sendComment() {
    if (!composer || !diff || draft.trim() === "" || sending) return;
    sending = true;
    actionError = null;
    try {
      const made = await app.backend.review.comment(number, { path: composer.path, line: composer.line, body: draft.trim() });
      comments = [...comments, ...withFields([made], diff)];
      composer = null;
      draft = "";
    } catch (e) {
      if (!authProblem(e)) actionError = message(e);
    } finally {
      sending = false;
    }
  }

  async function submit(verdict: ReviewVerdict) {
    if (sending) return;
    sending = true;
    actionError = null;
    sent = null;
    try {
      await app.backend.review.submit(number, { verdict, body: verdictBody.trim() });
      sent = VERDICT_DONE[verdict];
      verdictBody = "";
    } catch (e) {
      if (!authProblem(e)) actionError = message(e);
    } finally {
      sending = false;
    }
  }

  const statusOf = (s: RevisionRun["steps"][number]) => s.report.response?.status ?? null;
</script>

<div class="pull" data-testid="review-pull" data-number={number}>
  <div class="bar">
    <button class="button" data-testid="review-back" onclick={onback}>← All {word}s</button>
    {#if session}
      <strong class="title" data-testid="review-title">#{session.pull.number} {session.pull.title}</strong>
      <span class="badge" data-testid="review-state">{stateLabel(session.pull)}</span>
      <a class="muted" href={session.pull.url} target="_blank" rel="noopener noreferrer">Open on {repo.host}</a>
    {/if}
  </div>

  {#if loading}
    <p class="hint" data-testid="review-loading">Fetching the {word}…</p>
  {:else if error}
    <p class="error" role="alert" data-testid="review-pull-error">{error}</p>
  {:else if session && diff}
    <p class="muted branches" data-testid="review-branches">
      {session.pull.author} · {session.pull.source} → {session.pull.target} · {relativeTime(session.pull.updated)}
    </p>

    {#if ci}
      <div class="ci {ci.state}" data-testid="review-ci" data-state={ci.state}>
        <strong>{CI_LABEL[ci.state]}</strong>
        <ul>
          {#each ci.checks as c (c.name)}
            <li data-testid="review-check" data-state={c.state}>
              <span class="dot {c.state}" aria-hidden="true"></span>
              {#if c.url}<a href={c.url} target="_blank" rel="noopener noreferrer">{c.name}</a>{:else}{c.name}{/if}
              <span class="muted">{c.state}</span>
            </li>
          {/each}
        </ul>
      </div>
    {/if}

    <ContractNotes from={session.merge_base} to={session.head_ref} />
    <ExtensionDiffNotes from={session.merge_base} to={session.head_ref} />

    <section class="block" aria-label="Run the changed requests">
      <div class="head">
        <h3>Changed requests</h3>
        <span class="muted">{changed.length} {changed.length === 1 ? "request" : "requests"}</span>
        <span class="spacer"></span>
        <button class="button" data-testid="review-run" disabled={running || changed.length === 0} onclick={() => void runChanged()}>
          {running ? "Running…" : "Run at the head"}
        </button>
      </div>
      <p class="hint">
        Run from the {word}'s commit without a checkout, with {app.environment ? `your environment "${app.environment}"` : "no environment"},
        your secrets, cookies and plugins.
      </p>
      {#if runError}<p class="error" role="alert" data-testid="review-run-error">{runError}</p>{/if}
      {#if run}
        <ul class="steps" data-testid="review-run-result">
          {#each run.steps as s (s.request_path)}
            <li data-testid="review-step" data-path={s.request_path} data-passed={s.passed}>
              <span class={s.passed ? "ok" : "bad"} aria-label={s.passed ? "Passed" : "Failed"}>{s.passed ? "✓" : "✗"}</span>
              <span>{s.request_name}</span>
              <span class="muted mono">{s.request_path}</span>
              <span class="muted" data-testid="review-step-status">{statusOf(s) ?? s.report.error?.message ?? "no answer"}</span>
            </li>
          {/each}
        </ul>
        <p class="muted" data-testid="review-run-summary">{run.summary.passed} of {run.summary.requests} passed</p>
      {/if}
    </section>

    <section class="block" aria-label="Changes">
      <h3>Changes</h3>
      <SemanticDiff {diff} oncomment={startComment} />
    </section>

    <section class="block" aria-label="Comments">
      <h3>Comments</h3>
      {#if composer}
        <form
          class="composer"
          data-testid="review-composer"
          onsubmit={(e) => {
            e.preventDefault();
            void sendComment();
          }}
        >
          <label>
            Comment on <strong>{composer.label}</strong> <span class="muted mono">{composer.path}:{composer.line}</span>
            <textarea rows="3" data-testid="review-comment-text" bind:value={draft} disabled={sending}></textarea>
          </label>
          <div class="row">
            <button class="button primary" type="submit" data-testid="review-comment-send" disabled={sending || draft.trim() === ""}>Comment</button>
            <button class="button" type="button" onclick={() => (composer = null)}>Cancel</button>
          </div>
        </form>
      {/if}
      {#if comments.length === 0}
        <p class="hint" data-testid="review-no-comments">No comments yet. Use “Comment” next to a changed field.</p>
      {/if}
      {#each groups as g (g.file ?? "")}
        <div class="group" data-testid="review-comment-group" data-file={g.file ?? ""}>
          <h4>{g.file ?? "General"}</h4>
          <ul>
            {#each g.comments as c (c.id)}
              <li data-testid="review-comment" data-outdated={c.outdated}>
                <div class="who">
                  <strong>{c.author}</strong>
                  <span class="muted">{relativeTime(c.created)}</span>
                  {#if c.field}<span class="field" data-testid="review-comment-field">{fieldLabel(c.field)}</span>{/if}
                  {#if c.line !== null}<span class="muted mono">line {c.line}</span>{/if}
                  {#if c.outdated}<span class="badge">outdated</span>{/if}
                </div>
                <p class="text">{c.body}</p>
              </li>
            {/each}
          </ul>
        </div>
      {/each}
    </section>

    <section class="block" aria-label="Your review">
      <h3>Your review</h3>
      <textarea rows="3" placeholder="Summary (needed to request changes)" aria-label="Review summary" data-testid="review-body" bind:value={verdictBody} disabled={sending}></textarea>
      <div class="row">
        <button class="button primary" data-testid="review-approve" disabled={sending} onclick={() => void submit("approve")}>Approve</button>
        <button class="button" data-testid="review-request-changes" disabled={sending} onclick={() => void submit("request-changes")}>Request changes</button>
        <button class="button" data-testid="review-comment-only" disabled={sending || verdictBody.trim() === ""} onclick={() => void submit("comment")}>Comment</button>
      </div>
      {#if sent}<p class="ok" role="status" data-testid="review-sent">{sent}</p>{/if}
      {#if actionError}<p class="error" role="alert" data-testid="review-action-error">{actionError}</p>{/if}
    </section>
  {/if}
</div>

<style>
  .pull {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
    padding-bottom: 16px;
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-gap);
    padding: var(--rt-gap) 12px 0;
  }
  .title {
    overflow-wrap: anywhere;
  }
  .block,
  .ci {
    margin: 0 12px;
  }
  .block {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
  }
  .head {
    display: flex;
    align-items: baseline;
    gap: var(--rt-gap);
  }
  h3,
  h4 {
    margin: 0;
    font-size: var(--rt-size);
  }
  h4 {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
  p {
    margin: 0;
  }
  .branches,
  .hint,
  .error {
    padding: 0 12px;
  }
  .hint {
    padding: 0;
  }
  .branches {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
  }
  .muted,
  .hint {
    color: var(--rt-fg-muted);
  }
  .hint {
    font-size: var(--rt-size-small);
  }
  .error,
  .bad {
    color: var(--rt-danger);
  }
  .ok {
    color: var(--rt-ok);
  }
  .mono {
    font-family: var(--rt-font-mono);
  }
  .spacer {
    flex: 1;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--rt-gap);
  }
  .badge,
  .field {
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 0 6px;
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
  .button {
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    color: var(--rt-fg);
    cursor: pointer;
  }
  .button.primary:not(:disabled) {
    background: var(--rt-accent);
    border-color: var(--rt-accent);
    color: var(--rt-accent-fg);
  }
  .button:disabled {
    opacity: 0.6;
    cursor: default;
  }
  textarea {
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 4px 6px;
    width: 100%;
    box-sizing: border-box;
    resize: vertical;
  }
  .composer label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .composer {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
  }
  .ci {
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: var(--rt-gap) 12px;
  }
  .ci ul,
  .steps,
  .group ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .ci ul {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 16px;
    margin-top: 4px;
  }
  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--rt-fg-muted);
  }
  .dot.success {
    background: var(--rt-ok);
  }
  .dot.failure {
    background: var(--rt-danger);
  }
  .dot.pending {
    background: var(--rt-warn);
  }
  .steps li {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 12px;
    align-items: baseline;
  }
  .group ul {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
    margin-top: 4px;
  }
  .who {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 10px;
  }
  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
</style>
