// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Extensions of the app (wave 9): how a distribution adds its own commands, header items and
// start-up work to the shared UI without changing it. The builds of this repository pass none.
// Extensions are compiled into a distribution; plugins (WASM, `../plugins`) stay the way to
// extend the app at run time.

import type { Component } from "svelte";

import type { Backend } from "./commands/backend";
import type {
  AiDelta,
  AiProviderKind,
  AiReply,
  AiRequest,
  AiScope,
  BisectOutcome,
  GitConflict,
  GitWorkingCopy,
  RequestSpec,
  RunEvent,
  RunSummary,
  Script,
  ScriptPhase,
  StoredResponse,
  VarUse,
} from "./commands/types";
import type { AppState } from "./state/app.svelte";

/** What an extension sees: the backend the app was started with and the shared app state. */
export interface ExtensionContext {
  readonly backend: Backend;
  readonly app: AppState;
  /** The AI provider the user configured (since wave 16): every call masked, previewed when
   *  the settings ask for it, refused when the active collection is opted out. */
  readonly ai: ExtensionAi;
  /** Shows a suggested change for the user to apply or discard (since wave 16); resolves
   *  `true` once applied. Applied changes are unsaved, like any edit, and can be undone. */
  readonly suggest: (edit: SuggestedEdit) => Promise<boolean>;
  /** Like `suggest`, and tells whether the user chose "Don't suggest this again" (only offered
   *  for an edit with a `dismissKey`; since wave 17). The core stores nothing about it: the
   *  caller keeps the key. */
  readonly suggestOutcome: (edit: SuggestedEdit) => Promise<SuggestOutcome>;
}

/** What became of a suggestion (`ExtensionContext.suggestOutcome`). */
export interface SuggestOutcome {
  applied: boolean;
  /** The user asked not to be shown this suggestion again. Never with `applied`. */
  dismissed?: boolean;
}

// ---- Wave 16: AI for extensions, suggested edits, more places for actions ----

/** A configured profile as an extension sees it (never a key). */
export interface ExtensionAiProfile {
  id: string;
  label: string;
  provider: AiProviderKind;
  model: string;
  /** What the provider can do (`rt_app::ai::AiProviderKind::capabilities`). */
  tools: boolean;
  streaming: boolean;
}

export interface ExtensionAiStatus {
  /** At least one profile is set up. */
  configured: boolean;
  /** The active collection allows the assistant (`opencollection.yml` and this device). */
  allowed: boolean;
  profiles: ExtensionAiProfile[];
  /** The profiles the roles `fast` and `smart` resolve to, if any. */
  fast: string | null;
  smart: string | null;
}

/** A call: the extension's own prompt; the core adds the masked context of `scope`. */
export type ExtensionAiRequest = Omit<AiRequest, "id" | "scope"> & { scope?: AiScope };

export interface ExtensionAiOptions {
  /** Aborting cancels the call (the promise rejects with kind `cancelled`). */
  signal?: AbortSignal;
  /** Streamed pieces of the answer as they arrive. */
  onDelta?: (delta: AiDelta) => void;
  /** Show the "what will be sent" preview even when the settings do not require it. */
  preview?: boolean;
}

/** `ExtensionContext.ai`. Calls go only to the user's own provider through the app's transport;
 *  secret values in scope are masked before anything leaves. Errors are `BackendError`s with
 *  the kinds of `AiError` (`not-configured`, `no-key`, `opted-out`, `cancelled`, …). */
export interface ExtensionAi {
  status(): Promise<ExtensionAiStatus>;
  complete(request: ExtensionAiRequest, options?: ExtensionAiOptions): Promise<AiReply>;
}

/** Text for a suggestion: what it does and why (Markdown, short). */
export interface SuggestionNote {
  title?: string;
  explanation?: string;
}

/** A change shown as a diff or preview before it is applied (`ExtensionContext.suggest`). */
export type SuggestedEdit = SuggestedEditKind & {
  /** Offers "Don't suggest this again" in the dialog (since wave 17); choosing it reports
   *  `{ applied: false, dismissed: true }` from `suggestOutcome`. The caller remembers the key;
   *  the core stores nothing. */
  dismissKey?: string;
};

/** A variable an `environment` suggestion adds. A secret is declared without a value (the user
 *  enters it in the environment manager); `value` is ignored for it. */
export interface SuggestedVariable {
  name: string;
  value?: string;
  secret?: boolean;
}

export type SuggestedEditKind =
  /** The request at `path` replaced by `spec` (a field diff of the two). */
  | { kind: "request"; collection?: string; path: string; spec: RequestSpec; note?: SuggestionNote }
  /** A new request in `folder` (`""`: the root). */
  | { kind: "new-request"; collection?: string; folder: string; spec: RequestSpec; note?: SuggestionNote }
  /** The scripts of a folder (`path`) or of the collection (`path` `""`). */
  | { kind: "folder-scripts"; collection?: string; path: string; scripts: Script[]; note?: SuggestionNote }
  /** A text file of the collection that is not a request (e.g. `CHANGELOG.md`), staged like a
   *  companion file (`Backend.companions.stage`); `*.yml` is refused. */
  | { kind: "file"; collection?: string; path: string; text: string; note?: SuggestionNote }
  /** Variables added to the environment `environment` of the collection (since wave 17):
   *  unsaved, one undo step, usable as part of a `batch`. Variables the environment already has
   *  are left as they are, never overwritten; secrets get no value; an environment that does
   *  not exist is refused. */
  | { kind: "environment"; collection?: string; environment: string; variables: SuggestedVariable[]; note?: SuggestionNote }
  /** Several edits as one (since wave 17): one diff dialog, one undo step, applied entirely or
   *  not at all (e.g. a producer request and a consumer request). The edits are not batches
   *  themselves and all belong to `collection`; their own `collection` is ignored. */
  | { kind: "batch"; collection?: string; edits: SuggestedEditPart[]; note?: SuggestionNote };

/** An edit that can be part of a `batch`. */
export type SuggestedEditPart = Exclude<SuggestedEditKind, { kind: "batch" }>;

/** What `AppExtension.onExchange` gets (since wave 17). */
export interface ExchangeEvent {
  /** The collection that sent (`CollectionInfo.id`). */
  collection: string;
  /** The request's path. */
  path: string;
  /** The stored response: the report with its body preview and the handle to the full body. */
  response: StoredResponse;
  /** The request as it was sent, before variables were resolved (`{{variables}}` still in it). */
  request: RequestSpec;
  /** Where each variable of the request got its value from. */
  variables: VarUse[];
}

/** A code editor an action runs in (since wave 16). */
export type EditorTarget =
  /** A script editor: of a request (`path` a request), a folder (`path` a folder) or the
   *  collection (`path` `""`). */
  | { kind: "script"; collection: string; owner: "request" | "folder" | "collection"; path: string; phase: ScriptPhase }
  /** The body editor of a request. */
  | { kind: "body"; collection: string; path: string; contentType: string | null }
  /** Another code field of a request, e.g. `url`, `graphql-variables`. */
  | { kind: "field"; collection: string; path: string; field: string };

export interface EditorSelection {
  from: number;
  to: number;
}

/** What an editor action gets. */
export interface EditorHandle {
  readonly target: EditorTarget;
  /** The editor's text now, unsaved typing included. */
  text(): string;
  selection(): EditorSelection | null;
  /** Shows `next` as an inline diff against the current text, with Apply and Discard.
   *  Resolves `true` once applied (one undo step in the editor), `false` when discarded or
   *  when the text changed meanwhile. */
  propose(next: string, note?: SuggestionNote): Promise<boolean>;
}

/** An action in a code editor's toolbar or context menu (since wave 16). */
export interface ExtensionEditorAction {
  /** Unique within the extension. */
  id: string;
  label: string;
  /** An icon of the "New…" dialog's set. */
  icon?: string;
  /** A CodeMirror key, e.g. `Mod-i`; the first action that claims a key gets it. */
  shortcut?: string;
  /** Shown as a button in the editor's toolbar (else only in its menu). */
  inline?: boolean;
  run(editor: EditorHandle): unknown;
}

/** An action on a request, a response or a run (since wave 16). */
export interface ExtensionAction {
  /** Unique within the extension. */
  id: string;
  label: string;
  icon?: string;
  /** Shown as a button (at most two per place, the first ones); else in the "More" menu. */
  inline?: boolean;
  run: () => unknown;
}

/** The request shown in the request editor. */
export interface RequestActionTarget {
  collection: string;
  path: string;
  spec: RequestSpec;
}

/** The response shown in the response panel. */
export interface ResponseActionTarget {
  collection: string;
  path: string;
  response: StoredResponse;
  /** Status 400 and above, a transport error, or a failing test or assertion. */
  failed: boolean;
}

/** A finished run or bisect. */
export type RunActionTarget =
  | { kind: "run"; collection: string; folder: string; summary: RunSummary; events: readonly RunEvent[] }
  | { kind: "bisect"; collection: string; outcome: BisectOutcome };

/** Two revisions selected together (since wave 16): two commits or tags in the commit graph, or
 *  the sides of the comparison and of the diff of two revisions. */
export interface RevisionRangeTarget {
  /** The active collection, if the repository has one. */
  collection: string | null;
  /** Older side: a commit, branch or tag. */
  from: string;
  /** Newer side. */
  to: string;
  source: "graph" | "compare" | "diff";
}

/** What the assistant panel looks at: follows the active tab. */
export interface AssistantFocus {
  collection: string | null;
  /** The request of the active tab. */
  request: string | null;
  /** Its last response, if any. */
  response: StoredResponse | null;
  /** The folder selected in the sidebar, if any. */
  folder: string | null;
}

export interface AssistantPanelProps {
  ctx: ExtensionContext;
  focus: AssistantFocus;
  close: () => void;
}

/** A side panel of the main area (since wave 16), opened from the header and the palette. The
 *  shell shows the first extension's panel; it stays mounted while open. */
export interface ExtensionAssistant {
  title: string;
  icon?: string;
  component: Component<AssistantPanelProps>;
}

/** A command in the palette. */
export interface ExtensionCommand {
  /** Unique across extensions; prefixed with the extension's id by the shell. */
  id: string;
  label: string;
  run: () => unknown;
}

/** Where an extension tab is shown: among the request editor's tabs, among the response's tabs,
 *  or in the collection panel of the main area (header button "Collection", palette). */
export type ExtensionTabPlace = "request" | "response" | "collection";

/** What an extension tab component gets. */
export interface ExtensionTabProps {
  ctx: ExtensionContext;
  /** Path of the request the tab belongs to; `""` for collection tabs. */
  requestPath: string;
  /** Response tabs: the response shown (`response_id` for `backend.responseBytes`); else `null`. */
  response: StoredResponse | null;
  /** Hands the request editor's pending typing to the backend. Await it before reading the
   *  request (`backend.getRequest`) or replacing it (`app.edit`). Resolves at once outside
   *  request tabs. */
  settle: () => Promise<void>;
}

/** A tab rendered by the app like a plugin's tab, but as a Svelte component of a distribution. */
export interface ExtensionTab {
  /** Unique within the extension; the shell keys the tab `<extension id>/<tab id>`. */
  id: string;
  title: string;
  place: ExtensionTabPlace;
  /** Response tabs only: media types the tab is for, matched like a plugin tab's
   *  `content_types` (`application/json`, `application/*`; `application/xml` also matches
   *  `application/atom+xml`). Absent or empty: every response. */
  contentTypes?: readonly string[];
  component: Component<ExtensionTabProps>;
}

/** What a diff-notes component gets: the two revisions of a range (since wave 13). */
export interface ExtensionDiffNotesProps {
  ctx: ExtensionContext;
  /** Old side (a revision: branch, tag, commit). */
  from: string;
  /** New side. */
  to: string;
}

/** Where the "New…" dialog was opened for (since wave 14): the collection and folder its forms
 *  start with. `collection: null` when no collection is open. */
export interface NewTarget {
  collection: string | null;
  /** Folder path in that collection, `""` for its root. */
  folder: string;
}

/** A tile in the "New…" dialog (since wave 14), after the built-in ones. */
export interface ExtensionNewItem {
  /** Unique within the extension; the dialog keys the tile `<extension id>/<item id>`. */
  id: string;
  title: string;
  /** One short sentence under the title. */
  description: string;
  /** Group heading the tile appears under; tiles without one go under "More". Groups appear
   *  after the built-in ones, in the order they first occur. */
  group?: string;
  /** An icon of the dialog's set (`new/icons.ts`: `http`, `graphql`, `grpc`, `websocket`,
   *  `sse`, `collection`, `folder`, `environment`, `mcp`, `import`, `service`, `diagram`,
   *  `trace`, `spark`); unknown or absent: a generic tile icon. */
  icon?: string;
  /** A short word shown on the tile (at most 12 characters). */
  badge?: string;
  /** Runs after the dialog closed. */
  run(ctx: ExtensionContext, target: NewTarget): unknown;
}

/** What a hint is about (since wave 14). */
export type HintTarget =
  /** Above the request editor of `path` in collection `collection`. */
  | { place: "request"; collection: string; path: string; spec: RequestSpec }
  /** At the top of an expanded collection node in the sidebar. */
  | { place: "collection"; collection: string };

/** The Git views a hint can sit in (since wave 15). */
export type GitHintView = "graph" | "changes" | "conflict" | "history";

/** At the top of a Git view of repository `repository` (`AppExtension.gitHints`, since wave
 *  15); same rules as `HintTarget`. */
export interface GitHintTarget {
  place: "git";
  repository: string;
  view: GitHintView;
  // What the user is doing in the view, for hints that only fit one situation. The view that
  // shows the hint fills what it can tell; absent: unknown. Dismissals do not depend on them.
  /** `graph`: how many commits are selected together (1 = just the one, 2 = a range). */
  selected?: number;
  /** `graph`: a commit row is being dragged right now. */
  dragging?: boolean;
  /** `changes`: the largest number of hunks (separate blocks of changed lines) in one changed
   *  text file among the files shown so far; a file counts once it has been opened. */
  hunks?: number;
}

/** A small, calm note the shell shows in a defined place (since wave 14): never a modal, never
 *  blocking, always dismissible. The shell shows at most one hint per place at a time (the
 *  first not dismissed, extensions in order) and remembers a dismissal per device. */
export interface ExtensionHint {
  /** Unique within the extension; dismissals are stored under `<extension id>/<hint id>`. */
  id: string;
  /** One or two short sentences, plain text. */
  text: string;
  /** Up to two buttons; running one does not dismiss the hint unless `dismiss` is set. */
  actions?: readonly { label: string; run: () => unknown; dismiss?: boolean }[];
  /** `device` (default): once dismissed, never again on this device; `target`: dismissed for
   *  this collection or request only. */
  scope?: "device" | "target";
}

/** A collection or folder node of the sidebar (since wave 14). */
export type NodeTarget =
  | { kind: "collection"; collection: string }
  | { kind: "folder"; collection: string; path: string };

/** An entry in a collection or folder node's context menu (since wave 14). */
export interface ExtensionNodeAction {
  /** Unique within the extension. */
  id: string;
  label: string;
  /** Also shown as a small chip on the row itself (at most one per row: the first). */
  inline?: boolean;
  /** An icon of the "New…" dialog's set, for the chip. */
  icon?: string;
  run: () => unknown;
}

/** What a dialog component of an extension gets (since wave 14, `AppState.showDialog`). */
export interface ExtensionDialogProps {
  ctx: ExtensionContext;
  /** Closes the dialog; `showDialog` resolves with `result`. */
  close: (result?: unknown) => void;
  /** The `props` passed to `showDialog`. */
  props: Record<string, unknown>;
}

/** A dialog shown by the shell (since wave 14): title bar, close button, Escape and focus
 *  handling, light and dark theme, centred; one at a time. */
export interface ExtensionDialog {
  /** Title bar text and the dialog's accessible name. */
  title: string;
  component: Component<ExtensionDialogProps>;
  props?: Record<string, unknown>;
  /** Width in CSS pixels (default 560); never wider than the window minus 32 px. */
  width?: number;
}

// ---- Git views (since wave 15). Generic Git, nothing assumes a collection; paths are
// repository-relative with a leading `/`. ----

/** What a Git action of an extension is offered on. `repository` is `RepositoryEntry.id`. */
export type GitActionTarget =
  /** The repository as a whole: its sidebar node's menu, the Git panel's "Tools" section. */
  | { kind: "repository"; repository: string }
  /** One commit in the graph or a history list. */
  | { kind: "commit"; repository: string; commit: string }
  /** Several commits selected in the graph, newest first. */
  | { kind: "commits"; repository: string; commits: string[] }
  /** A branch, tag or remote branch (`GitRef.full`). */
  | { kind: "ref"; repository: string; ref: string }
  /** A file in the changes view, the file tree or a history list; `rev` absent: working tree. */
  | { kind: "file"; repository: string; path: string; rev?: string }
  /** The changed files of the working copy as a whole. */
  | { kind: "changes"; repository: string; paths: string[] }
  /** A parked change (`GitStash.index`). */
  | { kind: "stash"; repository: string; index: number }
  /** A conflicted file during a merge, cherry-pick, revert or rebase. */
  | { kind: "conflict"; repository: string; path: string };

/** A view in the main area's Git host, opened with `AppState.show({ kind: "git", view: {
 *  kind: "extension", id: "<extension>/<view>", props } })` (since wave 15). */
export interface ExtensionGitView {
  /** Unique within the extension. */
  id: string;
  /** Title bar of the view. */
  title: string;
  /** Semantic icon name (`ui/icons.ts`); default `graph`. */
  icon?: string;
  component: Component<ExtensionGitViewProps>;
}

export interface ExtensionGitViewProps {
  ctx: ExtensionContext;
  repository: string;
  /** The `props` the view was opened with. */
  props: Record<string, unknown>;
  /** Closes the view (back to the previous one). */
  close: () => void;
}

/** A staging view that replaces the file list of the app's commit form (since wave 15). With
 *  one, the form commits what the index holds (`GitApi.commitStaged`); without, the app keeps
 *  "commit without staging". The first extension's staging view wins. */
export interface ExtensionStaging {
  /** Shown as the file list's heading. */
  title: string;
  component: Component<ExtensionStagingProps>;
  /** Whether it is in charge for this repository right now (e.g. allowed to); called reactively:
   *  cheap, synchronous, no `$state` writes. Absent: always. */
  active?(ctx: ExtensionContext, repository: string): boolean;
}

export interface ExtensionStagingProps {
  ctx: ExtensionContext;
  repository: string;
  /** The working copy as last read; `refresh()` reads it again (after staging). */
  workingCopy: GitWorkingCopy;
  refresh: () => Promise<void>;
}

/** An editor for a file's diff or a conflicted file's merge, offered next to the app's own
 *  (since wave 15): the app's line and semantic views stay the default, the user can switch. */
export type ExtensionFileEditor =
  | {
      id: string;
      title: string;
      kind: "diff";
      /** Which files it can show; absent: all text files. */
      accepts?(path: string): boolean;
      component: Component<ExtensionDiffEditorProps>;
    }
  | {
      id: string;
      title: string;
      kind: "merge";
      accepts?(path: string): boolean;
      component: Component<ExtensionMergeEditorProps>;
    };

/** What a diff editor gets: a file's two sides, read-only context in the app's own view. The
 *  host shows it in place of its own diff (the user switches back with the same control);
 *  `close()` returns to the host's own diff. */
export interface ExtensionDiffEditorProps {
  ctx: ExtensionContext;
  repository: string;
  /** Repository-relative, with a leading `/`. */
  path: string;
  /** Revs of both sides; `to` `null`: the working tree. */
  from: string;
  to: string | null;
  close: () => void;
}

/**
 * What a merge editor gets. The contract of `resolve` and `close`:
 *
 * - `resolve(text)` writes `text` as the file's content and marks it resolved
 *   (`GitApi.resolveText`, which refuses while there are unsaved edits and says why), reloads
 *   the app's collections, and then **closes the editor itself**: the host returns to where the
 *   user came from. It resolves when all of that is done and rejects with the reason when not;
 *   the editor stays open then, so the user can retry or save first. Show the rejection's
 *   `message`.
 * - `close()` leaves the editor without resolving anything. Calling it after a successful
 *   `resolve` is harmless (the host closes once), so an editor may also call `close()` itself
 *   after `resolve` and does not have to.
 * - The editor is shown only for the file it was opened for; `conflict` holds the three
 *   versions (`base` the common ancestor, `ours`, `theirs`; absent = deleted on that side).
 */
export interface ExtensionMergeEditorProps {
  ctx: ExtensionContext;
  repository: string;
  conflict: GitConflict;
  resolve: (text: string) => Promise<void>;
  close: () => void;
}

/** The commit form an action works on (since wave 15). */
export interface CommitFormTarget {
  repository: string;
  /** The message field. */
  message: { get(): string; set(text: string): void };
  /** Files that will be committed (repository-relative). */
  paths: readonly string[];
  /** The form commits the index (a staging view is in charge). */
  staged: boolean;
}

export interface AppExtension {
  /** Stable id, e.g. `example`; also the prefix of its command ids (`example:…`). */
  readonly id: string;
  /**
   * Palette commands. Called inside a reactive scope, so it may read `ctx.app` and return
   * different commands as the state changes.
   */
  commands?(ctx: ExtensionContext): readonly ExtensionCommand[];
  /** Rendered at the end of the header, before the palette button, in the order given. */
  header?: Component<{ ctx: ExtensionContext }>;
  /** Rendered once at the end of the shell (dialogs, banners); shown or hidden by itself. */
  overlay?: Component<{ ctx: ExtensionContext }>;
  /** Runs once after the first frame; errors are logged, never shown. */
  start?(ctx: ExtensionContext): void | Promise<void>;
  /**
   * Called after every send of the app (since wave 17), with what went out and what came back.
   * An event, not a reactive scope: quick and synchronous, errors are logged, never shown. The
   * response is the masked report as the app shows it; `collection.observe.enabled: false`
   * (`extensions.requesttailor.observe`) stops the calls. The native shells' own observers are
   * `rt_core::observe::ExchangeObserver`.
   */
  onExchange?(ctx: ExtensionContext, event: ExchangeEvent): void;
  /**
   * Request, response and collection tabs (since wave 13). Called inside a reactive scope like
   * `commands`, so an extension can show tabs only while it is allowed to. Collection tabs
   * get palette entries ("Collection: <title>") like plugin collection tabs.
   */
  tabs?(ctx: ExtensionContext): readonly ExtensionTab[];
  /**
   * Notes on a range of revisions (since wave 13), shown after the OpenAPI contract notes in
   * the semantic diff of two revisions and in the pull-request review, e.g. the changes of
   * another description file. Renders nothing when it has nothing to say (or is not allowed
   * to); reads files with `ctx.backend.git.show`/`treeAt`.
   */
  diffNotes?: Component<ExtensionDiffNotesProps>;
  /**
   * Tiles of the "New…" dialog (since wave 14). Called inside a reactive scope like `commands`
   * while the dialog is open; must not write `$state`.
   */
  newItems?(ctx: ExtensionContext): readonly ExtensionNewItem[];
  /**
   * Hints for a place (since wave 14). Called inside a reactive scope for the shown request and
   * for every expanded collection node, so it must be cheap and synchronous (work from data
   * the extension prepared before, e.g. in `start`) and must not write `$state`.
   */
  hints?(ctx: ExtensionContext, target: HintTarget): readonly ExtensionHint[];
  /**
   * Context-menu entries of collection and folder nodes (since wave 14). Called for visible
   * nodes inside a reactive scope: cheap, synchronous, no `$state` writes.
   */
  nodeActions?(ctx: ExtensionContext, target: NodeTarget): readonly ExtensionNodeAction[];
  /**
   * Actions of code editors: script editors (request, folder, collection), the body editor and
   * other code fields (since wave 16). Called inside a reactive scope: cheap, synchronous, no
   * `$state` writes.
   */
  editorActions?(ctx: ExtensionContext, target: EditorTarget): readonly ExtensionEditorAction[];
  /** Actions on the request in the request editor (since wave 16); same rules. */
  requestActions?(ctx: ExtensionContext, target: RequestActionTarget): readonly ExtensionAction[];
  /** Actions on the shown response, e.g. after a failure (since wave 16); same rules. */
  responseActions?(ctx: ExtensionContext, target: ResponseActionTarget): readonly ExtensionAction[];
  /** Actions on a range of two revisions in the commit graph, the comparison and the diff of two
   *  revisions (since wave 16); same rules. */
  rangeActions?(ctx: ExtensionContext, target: RevisionRangeTarget): readonly ExtensionAction[];
  /** Actions on a finished run or bisect (since wave 16); same rules. */
  runActions?(ctx: ExtensionContext, target: RunActionTarget): readonly ExtensionAction[];
  /** A side panel, e.g. a chat about the current request (since wave 16). */
  assistant?: ExtensionAssistant;
  /**
   * Actions in the Git views (since wave 15): commits, selections of commits, refs, files,
   * the changes, parked changes, conflicts and the repository. Called inside a reactive scope:
   * cheap, synchronous, no `$state` writes. Entries with `inline` show as a button in the row.
   */
  gitActions?(ctx: ExtensionContext, target: GitActionTarget): readonly ExtensionAction[];
  /** Views in the main area's Git host (since wave 15); reactive like `commands`. */
  gitViews?(ctx: ExtensionContext): readonly ExtensionGitView[];
  /** A staging view for the commit form (since wave 15). */
  staging?: ExtensionStaging;
  /** Diff and merge editors offered next to the app's own (since wave 15); reactive. */
  fileEditors?(ctx: ExtensionContext): readonly ExtensionFileEditor[];
  /** Hints at the top of the Git views (since wave 15); same rules as `hints` (at most one per
   *  view, dismissible, remembered per device). */
  gitHints?(ctx: ExtensionContext, target: GitHintTarget): readonly ExtensionHint[];
  /** Actions next to the commit message (since wave 15), e.g. a message template; same rules
   *  as `gitActions`. */
  commitFormActions?(ctx: ExtensionContext, target: CommitFormTarget): readonly ExtensionAction[];
}

export interface StartOptions {
  extensions?: readonly AppExtension[];
}
