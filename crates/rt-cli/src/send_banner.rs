// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 17, strand D: what `rt send`, `rt run` and `rt send --watch` say on stderr.
//!
//! - The production banner: a visible hint when the environment is a production system
//!   (`rt_app::agents::is_production`). Never a refusal and never a different exit code, so
//!   existing pipelines keep working; the same text is a note in the report.
//! - The timing line (`--timing`): one stacked bar and one sentence, the text twin of the
//!   timing bar of the app (`apps/ui/src/response/timing-bar.ts`).

use rt_app::agents::is_production;
use rt_core::guard::{Effect, effect};
use rt_core::model::{AgentPolicy, Collection, RequestSpec};
use rt_core::send::{ReceivedResponse, SendReport};
use rt_core::services::{Note, NoteLevel};

/// Where the banner note comes from, in `SendReport.notes`.
pub const NOTE_SOURCE: &str = "production";

/// The banner for sending `spec` in `environment`, or `None` outside production.
pub fn banner(
    policy: &AgentPolicy,
    environment: Option<&str>,
    spec: &RequestSpec,
) -> Option<String> {
    let name = environment.filter(|_| is_production(policy, environment))?;
    Some(match effect(spec) {
        Effect::Write => format!(
            "PRODUCTION: this request writes ({}) to the environment {name}",
            spec.method
        ),
        Effect::Read => format!(
            "PRODUCTION: sending {} to the environment {name}",
            spec.method
        ),
    })
}

/// The banner for the request at `request_path` of `collection`, if any.
pub fn banner_for(
    collection: &Collection,
    request_path: &str,
    environment: Option<&str>,
) -> Option<String> {
    let context = collection.request(request_path)?;
    banner(
        &collection.config.agents,
        environment,
        &context.request.spec,
    )
}

/// The banner of `rt run` against `environment` (once, before the first request).
pub fn run_banner(
    policy: &AgentPolicy,
    environment: Option<&str>,
    requests: usize,
) -> Option<String> {
    let name = environment.filter(|_| is_production(policy, environment))?;
    Some(format!(
        "PRODUCTION: running {requests} request{} against the environment {name}",
        if requests == 1 { "" } else { "s" }
    ))
}

/// Adds the banner as a warning note of `report`.
pub fn add_note(report: &mut SendReport, banner: &str) {
    report.notes.push(Note {
        source: NOTE_SOURCE.into(),
        level: NoteLevel::Warning,
        text: banner.to_owned(),
    });
}

/// The phases of a response in milliseconds: DNS, connect, TLS (when measured), waiting for
/// the server, transfer; and the total.
struct Phases {
    parts: [(char, f64); 5],
    total: f64,
}

fn phases(r: &ReceivedResponse) -> Phases {
    let ms = |v: Option<std::time::Duration>| v.map_or(0.0, |d| d.as_secs_f64() * 1000.0);
    let (dns, connect, tls) = (ms(r.timing.dns), ms(r.timing.connect), ms(r.timing.tls));
    let known = dns + connect + tls;
    let ttfb = (r.timing.ttfb.as_secs_f64() * 1000.0).max(known);
    let total = (r.total.as_secs_f64() * 1000.0).max(ttfb);
    Phases {
        parts: [
            ('d', dns),
            ('c', connect),
            ('t', tls),
            ('w', ttfb - known),
            ('x', total - ttfb),
        ],
        total,
    }
}

const BAR_WIDTH: usize = 30;

/// `[ddcttwwwwwwwwwwxxxxx] Server wait is 82 % of 410 ms`: d = DNS, c = connect, t = TLS,
/// w = waiting for the server, x = transfer. `None` without a response.
pub fn timing_line(report: &SendReport) -> Option<String> {
    let p = phases(report.response.as_ref()?);
    let sum: f64 = p.parts.iter().map(|(_, v)| v).sum();
    let mut cells: Vec<(char, usize, f64)> = p
        .parts
        .iter()
        .map(|&(c, v)| {
            let exact = if sum > 0.0 {
                v / sum * BAR_WIDTH as f64
            } else {
                0.0
            };
            (c, exact.floor() as usize, exact.fract())
        })
        .collect();
    // Largest remainder: the cells add up to the width.
    let mut left = BAR_WIDTH.saturating_sub(cells.iter().map(|c| c.1).sum());
    let mut order: Vec<usize> = (0..cells.len()).collect();
    order.sort_by(|&a, &b| cells[b].2.total_cmp(&cells[a].2));
    for i in order {
        if left == 0 || sum <= 0.0 {
            break;
        }
        cells[i].1 += 1;
        left -= 1;
    }
    let bar: String = cells
        .iter()
        .flat_map(|&(c, n, _)| std::iter::repeat_n(c, n))
        .collect();
    let wait = p.parts[3].1;
    let percent = if p.total > 0.0 {
        (wait / p.total * 100.0).round()
    } else {
        0.0
    };
    Some(format!(
        "[{bar}] Server wait is {percent} % of {}",
        duration(p.total)
    ))
}

/// `12 ms`, `1.20 s` (as the app writes durations).
pub fn duration(ms: f64) -> String {
    if ms < 100.0 {
        format!("{} ms", format!("{ms:.1}").trim_end_matches(".0"))
    } else if ms < 1000.0 {
        format!("{} ms", ms.round())
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_core::model::{AgentAccess, EnvironmentPolicy};

    fn spec(method: &str) -> RequestSpec {
        RequestSpec {
            method: method.into(),
            ..Default::default()
        }
    }

    #[test]
    fn the_banner_names_environment_and_effect() {
        let policy = AgentPolicy {
            access: AgentAccess::Read,
            environments: vec![EnvironmentPolicy {
                environment: "Customer-A".into(),
                access: AgentAccess::Write,
                production: true,
            }],
        };
        let get = banner(&policy, Some("Customer-A"), &spec("GET")).unwrap();
        assert!(
            get.contains("sending GET") && get.contains("Customer-A"),
            "{get}"
        );
        let post = banner(&policy, Some("PRD"), &spec("POST")).unwrap();
        assert!(post.contains("writes (POST)"), "{post}");
        assert_eq!(banner(&policy, Some("Local"), &spec("GET")), None);
        assert_eq!(banner(&policy, None, &spec("GET")), None);
    }

    #[test]
    fn durations_read_like_the_apps() {
        assert_eq!(duration(0.5), "0.5 ms");
        assert_eq!(duration(12.0), "12 ms");
        assert_eq!(duration(410.2), "410 ms");
        assert_eq!(duration(1200.0), "1.20 s");
    }
}
