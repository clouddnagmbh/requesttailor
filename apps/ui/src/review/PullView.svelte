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
  import { Avatar, Badge, Banner, Button, Card, Chip, EmptyState, Spinner, TextArea } from "../ui";
  import { CI_LABEL, getGitViews, groupComments, relativeTime, requestWord, stateLabel } from "@tailor-kit/git-ui";
  import { changedRequestPaths, withFields } from "./pull-model";

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
  const views = getGitViews();

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
  <header class="bar">
    <Button variant="ghost" icon="chevron-left" data-testid="review-back" onclick={onback}>All {word}s</Button>
    {#if session}
      <h2 class="title" data-testid="review-title">#{session.pull.number} {session.pull.title}</h2>
      <span data-testid="review-state"><Badge tone={session.pull.state === "open" ? "ok" : session.pull.state === "merged" ? "accent" : "neutral"} value={stateLabel(session.pull)} /></span>
      <span class="spacer"></span>
      <a class="link" href={session.pull.url} target="_blank" rel="noopener noreferrer">Open on {repo.host}</a>
      <Button
        data-testid="review-compare"
        icon="compare"
        title="Every request of the {word} side by side, with the answers at both revisions"
        onclick={() => views.open({ kind: "compare", from: session!.merge_base, to: session!.head_ref })}>Compare side by side</Button
      >
    {/if}
  </header>

  {#if loading}
    <p class="loading" data-testid="review-loading"><Spinner size="sm" /> Fetching the {word}…</p>
  {:else if error}
    <div data-testid="review-pull-error"><Banner tone="danger">{error}</Banner></div>
  {:else if session && diff}
    <p class="meta" data-testid="review-branches">
      <Avatar name={session.pull.author} size="sm" />
      <span>{session.pull.author}</span>
      <Chip size="sm" mono icon="branch" label={session.pull.source} />
      <span aria-hidden="true">→</span>
      <Chip size="sm" mono label={session.pull.target} />
      <span>{relativeTime(session.pull.updated)}</span>
    </p>

    {#if ci}
      <div data-testid="review-ci" data-state={ci.state}>
        <Card title={CI_LABEL[ci.state]} padding="sm">
          <ul class="checks">
            {#each ci.checks as c (c.name)}
              <li data-testid="review-check" data-state={c.state}>
                <span class="dot {c.state}" aria-hidden="true"></span>
                {#if c.url}<a class="link" href={c.url} target="_blank" rel="noopener noreferrer">{c.name}</a>{:else}{c.name}{/if}
                <span class="muted">{c.state}</span>
              </li>
            {/each}
          </ul>
        </Card>
      </div>
    {/if}

    <ContractNotes from={session.merge_base} to={session.head_ref} />
    <ExtensionDiffNotes from={session.merge_base} to={session.head_ref} />

    <section aria-label="Run the changed requests">
      <Card title="Changed requests" description="{changed.length} {changed.length === 1 ? 'request' : 'requests'}">
        {#snippet actions()}
          <Button variant="primary" size="sm" icon="play" data-testid="review-run" loading={running} disabled={running || changed.length === 0} onclick={() => void runChanged()}>
            {running ? "Running…" : "Run at the head"}
          </Button>
        {/snippet}
        <div class="stack">
          <p class="hint">
            Run from the {word}'s commit without a checkout, with {app.environment ? `your environment "${app.environment}"` : "no environment"},
            your secrets, cookies and plugins.
          </p>
          {#if runError}<div data-testid="review-run-error"><Banner tone="danger">{runError}</Banner></div>{/if}
          {#if run}
            <ul class="steps" data-testid="review-run-result">
              {#each run.steps as s (s.request_path)}
                <li data-testid="review-step" data-path={s.request_path} data-passed={s.passed}>
                  <span class={s.passed ? "ok" : "bad"} aria-label={s.passed ? "Passed" : "Failed"}>{s.passed ? "✓" : "✗"}</span>
                  <span class="name">{s.request_name}</span>
                  <span class="muted mono">{s.request_path}</span>
                  <span class="spacer"></span>
                  <span class="muted" data-testid="review-step-status">{statusOf(s) ?? s.report.error?.message ?? "no answer"}</span>
                </li>
              {/each}
            </ul>
            <p class="muted" data-testid="review-run-summary">{run.summary.passed} of {run.summary.requests} passed</p>
          {/if}
        </div>
      </Card>
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
          <Card padding="sm">
            <div class="stack">
              <TextArea
                rows={3}
                data-testid="review-comment-text"
                bind:value={draft}
                disabled={sending}
                label="Comment on {composer.label} · {composer.path}:{composer.line}"
              />
              <div class="row">
                <Button variant="primary" type="submit" data-testid="review-comment-send" disabled={sending || draft.trim() === ""}>Comment</Button>
                <Button variant="ghost" onclick={() => (composer = null)}>Cancel</Button>
              </div>
            </div>
          </Card>
        </form>
      {/if}
      {#if comments.length === 0}
        <div data-testid="review-no-comments"><EmptyState size="sm" icon="review" title="No comments yet." text="Use “Comment” next to a changed field." /></div>
      {/if}
      {#each groups as g (g.file ?? "")}
        <div class="group" data-testid="review-comment-group" data-file={g.file ?? ""}>
          <h4>{g.file ?? "General"}</h4>
          <ul>
            {#each g.comments as c (c.id)}
              <li data-testid="review-comment" data-outdated={c.outdated}>
                <Card padding="sm">
                  <div class="who">
                    <Avatar name={c.author} size="sm" />
                    <strong>{c.author}</strong>
                    <span class="muted">{relativeTime(c.created)}</span>
                    {#if c.field}<span data-testid="review-comment-field"><Chip size="sm" label={fieldLabel(c.field)} /></span>{/if}
                    {#if c.line !== null}<span class="muted mono">line {c.line}</span>{/if}
                    {#if c.outdated}<Badge tone="warn" value="outdated" />{/if}
                  </div>
                  <p class="text">{c.body}</p>
                </Card>
              </li>
            {/each}
          </ul>
        </div>
      {/each}
    </section>

    <section aria-label="Your review">
      <Card title="Your review">
        <div class="stack">
          <TextArea rows={3} placeholder="Summary (needed to request changes)" aria-label="Review summary" data-testid="review-body" bind:value={verdictBody} disabled={sending} />
          <div class="row">
            <Button variant="primary" data-testid="review-approve" disabled={sending} onclick={() => void submit("approve")}>Approve</Button>
            <Button data-testid="review-request-changes" disabled={sending} onclick={() => void submit("request-changes")}>Request changes</Button>
            <Button variant="ghost" data-testid="review-comment-only" disabled={sending || verdictBody.trim() === ""} onclick={() => void submit("comment")}>Comment</Button>
          </div>
          {#if sent}<div data-testid="review-sent"><Banner tone="ok">{sent}</Banner></div>{/if}
          {#if actionError}<div data-testid="review-action-error"><Banner tone="danger">{actionError}</Banner></div>{/if}
        </div>
      </Card>
    </section>
  {/if}
</div>

<style>
  .pull {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-6);
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-4);
  }
  h2 {
    margin: 0;
    font: var(--rt-font-heading);
    overflow-wrap: anywhere;
    min-width: 0;
  }
  h3 {
    margin: 0;
    font: var(--rt-font-title);
  }
  h4 {
    margin: 0;
    font: var(--rt-font-small);
    font-family: var(--rt-font-mono);
    color: var(--rt-fg-muted);
  }
  p {
    margin: 0;
  }
  .link {
    color: var(--rt-accent);
    font: var(--rt-font-small);
  }
  .spacer {
    flex: 1;
  }
  .loading,
  .meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-3);
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .block,
  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
  }
  .muted,
  .hint {
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .bad {
    color: var(--rt-danger-fg);
  }
  .ok {
    color: var(--rt-ok-fg);
  }
  .mono {
    font-family: var(--rt-font-mono);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--rt-space-4);
  }
  .checks,
  .steps,
  .group ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .checks {
    display: flex;
    flex-wrap: wrap;
    gap: var(--rt-space-2) var(--rt-space-6);
  }
  .checks li {
    display: inline-flex;
    align-items: center;
    gap: var(--rt-space-3);
  }
  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: var(--rt-radius-pill);
    background: var(--rt-fg-muted);
  }
  .dot.success {
    background: var(--rt-ok-fg);
  }
  .dot.failure {
    background: var(--rt-danger-fg);
  }
  .dot.pending {
    background: var(--rt-warn-fg);
  }
  .steps {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
  }
  .steps li {
    display: flex;
    flex-wrap: wrap;
    gap: var(--rt-space-2) var(--rt-space-5);
    align-items: baseline;
    padding: var(--rt-space-3) 0;
    border-top: 1px solid var(--rt-border);
  }
  .steps li:first-child {
    border-top: 0;
  }
  .name {
    font: var(--rt-font-body);
  }
  .group ul {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
    margin-top: var(--rt-space-3);
  }
  .who {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-3) var(--rt-space-4);
    margin-bottom: var(--rt-space-3);
  }
  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
</style>
