// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The halving of `rt_app::bisect::search`: counts of tests, skipped commits, edge cases.

use std::cell::RefCell;
use std::future::Future;
use std::task::{Context, Poll};

use rt_app::bisect::{BisectVerdict, search};
use rt_app::git::GitCommit;

fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = std::pin::pin!(fut);
    let mut cx = Context::from_waker(std::task::Waker::noop());
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

/// `n` commits, newest first, `c<n>` … `c1`.
fn chain(n: u32) -> Vec<GitCommit> {
    (1..=n)
        .rev()
        .map(|i| GitCommit {
            oid: format!("c{i}"),
            summary: String::new(),
            author: String::new(),
            time: i64::from(i),
        })
        .collect()
}

fn id(c: &GitCommit) -> u32 {
    c.oid[1..].parse().unwrap()
}

/// Runs a search where `verdict(i)` says what commit `i` is; returns the outcome and the tested ids.
fn run(
    n: u32,
    verdict: impl Fn(u32) -> BisectVerdict,
) -> (rt_app::bisect::BisectOutcome, Vec<u32>) {
    let tested = RefCell::new(Vec::new());
    let outcome = block_on(search(
        chain(n),
        async |c: &GitCommit| {
            tested.borrow_mut().push(id(c));
            let v = verdict(id(c));
            (
                v,
                if v == BisectVerdict::Bad {
                    vec!["x".into()]
                } else {
                    Vec::new()
                },
            )
        },
        &mut |_| {},
        &|| false,
    ));
    (outcome, tested.into_inner())
}

fn threshold(first_bad: u32) -> impl Fn(u32) -> BisectVerdict {
    move |i| {
        if i >= first_bad {
            BisectVerdict::Bad
        } else {
            BisectVerdict::Good
        }
    }
}

#[test]
fn finds_every_first_bad_commit_in_log_steps() {
    for first_bad in 1..=49 {
        let (outcome, tested) = run(49, threshold(first_bad));
        assert_eq!(outcome.first_bad.unwrap().oid, format!("c{first_bad}"));
        assert_eq!(outcome.candidates, 49);
        assert!(tested.len() <= 7, "{first_bad}: {tested:?}");
        assert_eq!(outcome.steps.len(), tested.len());
    }
}

#[test]
fn all_good_finds_nothing_after_one_run() {
    let (outcome, tested) = run(10, |_| BisectVerdict::Good);
    assert!(outcome.first_bad.is_none());
    assert_eq!(tested, [10]);
}

#[test]
fn all_bad_finds_the_oldest_candidate() {
    let (outcome, _) = run(10, |_| BisectVerdict::Bad);
    assert_eq!(outcome.first_bad.unwrap().oid, "c1");
}

#[test]
fn no_candidates_is_an_empty_outcome() {
    let (outcome, tested) = run(0, |_| BisectVerdict::Bad);
    assert!(outcome.first_bad.is_none() && outcome.steps.is_empty() && tested.is_empty());
    assert_eq!(outcome.candidates, 0);
}

#[test]
fn skipped_commits_are_stepped_around() {
    // The middle ones cannot be tested; the first bad one is still found.
    let (outcome, tested) = run(20, |i| {
        if (9..=12).contains(&i) {
            BisectVerdict::Skip
        } else {
            threshold(15)(i)
        }
    });
    assert_eq!(outcome.first_bad.unwrap().oid, "c15");
    assert!(tested.iter().any(|i| (9..=12).contains(i)));
    assert!(
        outcome
            .steps
            .iter()
            .any(|s| s.verdict == BisectVerdict::Skip)
    );
}

#[test]
fn only_skipped_commits_left_gives_no_answer() {
    // c1..c5 good, c6..c8 skipped, c9.. bad: the first bad one is among the skipped.
    let (outcome, _) = run(12, |i| match i {
        1..=5 => BisectVerdict::Good,
        6..=8 => BisectVerdict::Skip,
        _ => BisectVerdict::Bad,
    });
    assert!(outcome.first_bad.is_none());
}

#[test]
fn a_skipped_tip_gives_no_answer() {
    let (outcome, tested) = run(5, |_| BisectVerdict::Skip);
    assert!(outcome.first_bad.is_none());
    assert_eq!(tested, [5]);
}

#[test]
fn steps_report_the_candidates_left() {
    let steps = RefCell::new(Vec::new());
    let outcome = block_on(search(
        chain(16),
        async |c: &GitCommit| (threshold(5)(id(c)), Vec::new()),
        &mut |s| steps.borrow_mut().push(s.clone()),
        &|| false,
    ));
    let steps = steps.into_inner();
    assert_eq!(steps, outcome.steps);
    assert_eq!(steps[0].commit.oid, "c16");
    // Strictly shrinking, ending at nothing left.
    let remaining: Vec<_> = steps.iter().map(|s| s.remaining).collect();
    assert!(remaining.windows(2).all(|w| w[0] >= w[1]), "{remaining:?}");
    assert_eq!(*remaining.last().unwrap(), 0);
}

#[test]
fn cancel_stops_before_the_next_test() {
    let tested = RefCell::new(0u32);
    let outcome = block_on(search(
        chain(30),
        async |_: &GitCommit| {
            *tested.borrow_mut() += 1;
            (BisectVerdict::Bad, Vec::new())
        },
        &mut |_| {},
        &|| *tested.borrow() >= 2,
    ));
    assert_eq!(*tested.borrow(), 2);
    assert!(outcome.first_bad.is_none());
    // The second test was cut short: only the first is reported.
    assert_eq!(outcome.steps.len(), 1);
}
