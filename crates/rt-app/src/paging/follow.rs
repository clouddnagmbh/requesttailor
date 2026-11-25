// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The run: send the first page, find the next one, send it, and so on, within the limits.
//! One implementation for the app, the browser and `rt send --all-pages`; the caller supplies
//! how one request is sent (every page is a full `send_with`: variables, auth, the send guard,
//! scripts and plugin hooks), so this module sends nothing itself.

use std::collections::HashSet;

use rt_core::guard::{Effect, effect};
use rt_core::model::RequestSpec;
use rt_core::runner::platform_sleep;
use rt_core::send::{ReportBody, SendError, SendReport};
use rt_core::services::{Note, NoteLevel};
use url::Url;
use web_time::Instant;

use super::merge::Merger;
use super::next::{Next, body_text, cursor_text, next_of, origin, resolve, with_cursor};
use super::{PagedRun, PagingLimits, PagingMode, PagingRule, PagingSpec, PagingStop, PreviousPage};
use crate::AppError;

const SOURCE: &str = "paging";

/// What one run needs.
pub struct Follow<'a> {
    /// The request as the first page is sent (and the base of every later page).
    pub spec: &'a RequestSpec,
    pub rule: &'a PagingSpec,
    pub mode: PagingMode,
    /// `Next` only: the page to continue from.
    pub previous: Option<PreviousPage>,
    pub limits: PagingLimits,
    /// `All` only: the first page, if the caller sent it already (to learn which rule
    /// applies); it is not sent again.
    pub first: Option<SendReport>,
}

fn note(level: NoteLevel, text: impl Into<String>) -> Note {
    Note {
        source: SOURCE.to_owned(),
        level,
        text: text.into(),
    }
}

/// Why a page that came back stops the run (`None`: it is fine).
fn problem(report: &SendReport) -> Option<PagingStop> {
    match (&report.error, &report.response) {
        (Some(SendError::Blocked { rule, reason }), _) => {
            Some(PagingStop::Refused(format!("{rule}: {reason}")))
        }
        (Some(error), _) => Some(PagingStop::Refused(describe(error))),
        (None, None) => Some(PagingStop::ErrorStatus(0)),
        (None, Some(r)) if r.status >= 400 => Some(PagingStop::ErrorStatus(r.status)),
        _ => None,
    }
}

fn reason(stop: &PagingStop) -> String {
    match stop {
        PagingStop::Done => "no further page".into(),
        PagingStop::MaxPages => "limit reached".into(),
        PagingStop::MaxBytes => "size limit reached".into(),
        PagingStop::MaxSeconds => "time limit reached".into(),
        PagingStop::Loop => "the next page is one already loaded".into(),
        PagingStop::OtherOrigin(m) => m.clone(),
        PagingStop::ErrorStatus(0) => "a page got no answer".into(),
        PagingStop::ErrorStatus(s) => format!("a page answered {s}"),
        PagingStop::Refused(m) => format!("a page was refused: {m}"),
        PagingStop::NotRead => "the request writes".into(),
    }
}

fn describe(error: &SendError) -> String {
    match error {
        SendError::NotFound(m)
        | SendError::Prepare(m)
        | SendError::Auth(m)
        | SendError::Plugin(m)
        | SendError::Script(m) => m.clone(),
        SendError::Blocked { rule, reason } => format!("{rule}: {reason}"),
        SendError::Transport(e) => e.to_string(),
    }
}

fn request_url(report: &SendReport) -> Option<&str> {
    report.request.as_ref().map(|r| r.url.as_str())
}

fn response_url(report: &SendReport) -> Option<&str> {
    report.response.as_ref().map(|r| r.url.as_str())
}

/// How long to wait before the next page, from what the last response said about its limits.
fn wait_ms(report: &SendReport) -> u64 {
    let Some(limits) = &report.rate_limit else {
        return 0;
    };
    if let Some(ms) = limits.retry_after_ms {
        return ms;
    }
    if limits.remaining == Some(0) {
        return limits.reset_ms.unwrap_or(0);
    }
    0
}

/// Sleeps `ms`, in slices so that `cancelled` is seen within a quarter of a second.
async fn wait(ms: u64, cancelled: &dyn Fn() -> bool) {
    let mut left = ms;
    while left > 0 && !cancelled() {
        let slice = left.min(250);
        platform_sleep(slice).await;
        left -= slice;
    }
}

/// The request for the page `next` names, or why there is none: the stop and whether pages
/// are left.
fn page_spec(
    base: &RequestSpec,
    rule: &PagingRule,
    next: Next,
    current: &SendReport,
    first_origin: &str,
    seen: &mut HashSet<String>,
) -> Result<RequestSpec, PagingStop> {
    match next {
        Next::None => unreachable!("no next page is handled by the caller"),
        Next::Url(target) => {
            let base_url = response_url(current)
                .or_else(|| request_url(current))
                .unwrap_or_default();
            let url = resolve(base_url, &target)
                .map_err(|m| PagingStop::Refused(format!("the next page is unusable: {m}")))?;
            if origin(&url) != first_origin {
                return Err(PagingStop::OtherOrigin(format!(
                    "the next page is at {}, not at {first_origin}: pages stay on the origin of the request",
                    origin(&url)
                )));
            }
            if !seen.insert(url.as_str().to_owned()) {
                return Err(PagingStop::Loop);
            }
            let mut spec = base.clone();
            spec.url = url.as_str().to_owned();
            Ok(spec)
        }
        Next::Cursor(value) => {
            let PagingRule::Cursor { param, .. } = rule else {
                return Err(PagingStop::Refused("the rule names no cursor".into()));
            };
            if !seen.insert(cursor_text(&value)) {
                return Err(PagingStop::Loop);
            }
            with_cursor(base, param, &value)
                .map_err(|m| PagingStop::Refused(format!("the cursor cannot be placed: {m}")))
        }
    }
}

fn finish(
    mut report: SendReport,
    pages: u32,
    stopped: PagingStop,
    more: bool,
    merged: Option<String>,
    extra: Vec<Note>,
) -> PagedRun {
    if let (Some(text), Some(response)) = (merged, report.response.as_mut()) {
        response.size = text.len() as u64;
        response.truncated = false;
        response.body = ReportBody::Utf8(text);
    }
    report.notes.extend(extra);
    let summary = match (&stopped, pages) {
        (PagingStop::Done, 1) => "1 page".to_owned(),
        (PagingStop::Done, n) => format!("{n} pages"),
        (stop, n) => {
            let tail = if more { "; more pages exist" } else { "" };
            format!("stopped after {n} pages: {}{tail}", reason(stop))
        }
    };
    let level = if matches!(stopped, PagingStop::Done) {
        NoteLevel::Info
    } else {
        NoteLevel::Warning
    };
    report.notes.push(note(level, summary));
    PagedRun {
        report,
        pages,
        stopped,
        more,
    }
}

/// Follows the pages. `send` sends one request through the whole send path and gives the
/// report; `Err` is a reason the run cannot go on at all (the page is not sent). `on_page` sees
/// every page report as it arrives (the body may be large: look, don't keep).
///
/// `Err` of this function: the request is not allowed to follow pages (it writes) or the rule
/// is unusable. A page that cannot be had ends the run with a partial result instead.
pub async fn follow<F>(
    run: Follow<'_>,
    mut send: F,
    cancelled: &dyn Fn() -> bool,
    on_page: &mut dyn FnMut(&SendReport),
) -> Result<PagedRun, AppError>
where
    F: AsyncFnMut(RequestSpec) -> Result<SendReport, String>,
{
    super::rules::validate(run.rule)?;
    if effect(run.spec) != Effect::Read {
        return Err(AppError::Shell(
            "Only requests that read follow pages: this one writes.".into(),
        ));
    }
    let limits = run.limits.clamped();
    let started = Instant::now();
    let rule = &run.rule.rule;
    let mut seen: HashSet<String> = HashSet::new();
    let mut extra: Vec<Note> = Vec::new();

    // The page everything starts from.
    let (mut current, first_origin, mut pages, one_page) = match run.mode {
        PagingMode::All => {
            let page = match run.first {
                Some(page) => page,
                None => match send(run.spec.clone()).await {
                    Ok(page) => page,
                    Err(m) => return Err(AppError::Shell(m)),
                },
            };
            if let Some(stop) = problem(&page) {
                on_page(&page);
                return Ok(finish(page, 1, stop, false, None, extra));
            }
            let Some(url) = request_url(&page).and_then(|u| Url::parse(u).ok()) else {
                return Ok(finish(page, 1, PagingStop::Done, false, None, extra));
            };
            seen.insert(url.as_str().to_owned());
            let first_origin = origin(&url);
            (page, first_origin, 1u32, false)
        }
        PagingMode::Next => {
            let prev = run
                .previous
                .ok_or_else(|| AppError::Shell("There is no page to continue from.".into()))?;
            let mut page = prev.report;
            // The kept report has a preview of the body; the full text is what the rule reads.
            if let Some(response) = page.response.as_mut() {
                response.body = match String::from_utf8(prev.body) {
                    Ok(text) => ReportBody::Utf8(text),
                    Err(e) => ReportBody::Base64(base64_of(e.as_bytes())),
                };
            }
            let url = request_url(&page)
                .and_then(|u| Url::parse(u).ok())
                .ok_or_else(|| AppError::Shell("The page has no URL to continue from.".into()))?;
            seen.insert(url.as_str().to_owned());
            let first_origin = origin(&url);
            (page, first_origin, 0u32, true)
        }
    };

    let mut merger = None;
    let mut total_bytes = current
        .response
        .as_ref()
        .map_or(0, |r| r.size.max(body_len(&r.body)));
    if !one_page && let Some(path) = &run.rule.merge {
        match Merger::new(&body_text(&current), path) {
            Ok(m) => merger = Some(m),
            Err(m) => {
                on_page(&current);
                extra.push(note(NoteLevel::Warning, format!("Not following: {m}.")));
                return Ok(finish(current, 1, PagingStop::Done, false, None, extra));
            }
        }
    }
    if !one_page {
        on_page(&current);
    }
    // The first page's report carries the merged result; its body lives in the merger now.
    let mut first_report: Option<SendReport> = None;
    if merger.is_some() {
        let saved = current
            .response
            .as_mut()
            .map(|r| std::mem::replace(&mut r.body, ReportBody::Utf8(String::new())));
        let kept = current.clone();
        if let (Some(r), Some(body)) = (current.response.as_mut(), saved) {
            r.body = body;
        }
        first_report = Some(kept);
    }

    let (stopped, more) = loop {
        let next = match next_of(rule, &current) {
            Ok(Next::None) => break (PagingStop::Done, false),
            Ok(next) => next,
            Err(m) => {
                extra.push(note(
                    NoteLevel::Warning,
                    format!("The rule does not apply to this page: {m}."),
                ));
                break (PagingStop::Done, false);
            }
        };
        if one_page && pages >= 1 {
            break (PagingStop::Done, true);
        }
        if pages >= limits.max_pages {
            break (PagingStop::MaxPages, true);
        }
        if cancelled() {
            break (PagingStop::Refused("cancelled".into()), true);
        }
        let elapsed = started.elapsed().as_millis() as u64;
        let budget = limits.max_seconds.saturating_mul(1_000);
        if elapsed >= budget {
            break (PagingStop::MaxSeconds, true);
        }
        let pause = wait_ms(&current);
        if pause > 0 {
            if elapsed + pause > budget {
                break (PagingStop::MaxSeconds, true);
            }
            wait(pause, cancelled).await;
            if cancelled() {
                break (PagingStop::Refused("cancelled".into()), true);
            }
        }
        let spec = match page_spec(run.spec, rule, next, &current, &first_origin, &mut seen) {
            Ok(spec) => spec,
            Err(stop) => break (stop, true),
        };
        let page = match send(spec).await {
            Ok(page) => page,
            Err(m) => break (PagingStop::Refused(m), true),
        };
        if let Some(stop) = problem(&page) {
            on_page(&page);
            break (stop, true);
        }
        let size = page
            .response
            .as_ref()
            .map_or(0, |r| r.size.max(body_len(&r.body)));
        if total_bytes + size > limits.max_bytes {
            break (PagingStop::MaxBytes, true);
        }
        total_bytes += size;
        pages += 1;
        if let Some(m) = merger.as_mut() {
            let text = body_text(&page);
            if let Err(why) = m.add(&text) {
                extra.push(note(
                    NoteLevel::Warning,
                    format!("Page {pages} was not merged: {why}."),
                ));
                break (PagingStop::Done, true);
            }
        }
        on_page(&page);
        current = page;
    };

    if one_page {
        // One page more: it is the result, as a page of its own.
        return Ok(finish(current, pages.max(1), stopped, more, None, extra));
    }
    let merged = merger.map(Merger::finish);
    let result = match (merged.is_some(), first_report) {
        (true, Some(first)) => first,
        _ => current,
    };
    Ok(finish(result, pages, stopped, more, merged, extra))
}

fn body_len(body: &ReportBody) -> u64 {
    match body {
        ReportBody::Utf8(s) => s.len() as u64,
        ReportBody::Base64(s) => s.len() as u64 / 4 * 3,
    }
}

fn base64_of(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}
