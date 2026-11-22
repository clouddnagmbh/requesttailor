// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Line-level three-way merge (diff3) for files the model does not cover: what Git's own text
//! merge does, in memory, so the web app, the native pull and `rt merge-driver` agree.
//!
//! Changes of both sides that touch or overlap the same base lines conflict unless they are
//! identical. A conflict is written with Git's markers (`<<<<<<< ours`, `=======`,
//! `>>>>>>> theirs`).

use similar::{Algorithm, DiffTag, capture_diff_slices};

/// The result of [`merge_lines`].
pub(crate) struct TextMerge {
    pub text: String,
    /// No conflicting hunks.
    pub clean: bool,
}

/// One changed range: base lines `base` are replaced by the side's lines `side`.
#[derive(Clone, Copy)]
struct Hunk {
    base: (usize, usize),
    side: (usize, usize),
}

fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// The changed ranges of `side` against `base`, adjacent operations joined.
fn hunks(base: &[&str], side: &[&str]) -> Vec<Hunk> {
    let mut out: Vec<Hunk> = Vec::new();
    for op in capture_diff_slices(Algorithm::Myers, base, side) {
        let (tag, b, s) = op.as_tag_tuple();
        if tag == DiffTag::Equal {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.base.1 == b.start && last.side.1 == s.start => {
                last.base.1 = b.end;
                last.side.1 = s.end;
            }
            _ => out.push(Hunk {
                base: (b.start, b.end),
                side: (s.start, s.end),
            }),
        }
    }
    out
}

/// The side's text for base lines `from..to`, given the side's hunks inside that range.
fn region<'a>(base: &[&'a str], side: &[&'a str], hs: &[Hunk], from: usize, to: usize) -> String {
    let (Some(first), Some(last)) = (hs.first(), hs.last()) else {
        return base[from..to].concat();
    };
    let start = first.side.0 - (first.base.0 - from);
    let end = last.side.1 + (to - last.base.1);
    side[start..end].concat()
}

/// Merges `ours` and `theirs`, both edited from `base`, line by line.
pub(crate) fn merge_lines(base: &str, ours: &str, theirs: &str) -> TextMerge {
    let (b, o, t) = (lines(base), lines(ours), lines(theirs));
    let (ho, ht) = (hunks(&b, &o), hunks(&b, &t));
    let mut out = String::with_capacity(ours.len().max(theirs.len()));
    let mut clean = true;
    let (mut i, mut j) = (0, 0);
    let mut pos = 0;
    while i < ho.len() || j < ht.len() {
        // Start a group with the hunk that begins first, then pull in everything of either
        // side that touches the group's base range.
        let start = match (ho.get(i), ht.get(j)) {
            (Some(x), Some(y)) => x.base.0.min(y.base.0),
            (Some(x), None) => x.base.0,
            (None, Some(y)) => y.base.0,
            (None, None) => unreachable!(),
        };
        let mut end = start;
        let (gi, gj) = (i, j);
        loop {
            let mut grew = false;
            while let Some(h) = ho.get(i).filter(|h| h.base.0 <= end) {
                end = end.max(h.base.1);
                i += 1;
                grew = true;
            }
            while let Some(h) = ht.get(j).filter(|h| h.base.0 <= end) {
                end = end.max(h.base.1);
                j += 1;
                grew = true;
            }
            if !grew {
                break;
            }
        }
        out.push_str(&b[pos..start].concat());
        let (go, gt) = (&ho[gi..i], &ht[gj..j]);
        let ours_text = region(&b, &o, go, start, end);
        let theirs_text = region(&b, &t, gt, start, end);
        if go.is_empty() {
            out.push_str(&theirs_text);
        } else if gt.is_empty() || ours_text == theirs_text {
            out.push_str(&ours_text);
        } else {
            clean = false;
            let nl = if ours.contains("\r\n") { "\r\n" } else { "\n" };
            let side = |out: &mut String, text: &str| {
                out.push_str(text);
                if !text.is_empty() && !text.ends_with('\n') {
                    out.push_str(nl);
                }
            };
            out.push_str("<<<<<<< ours");
            out.push_str(nl);
            side(&mut out, &ours_text);
            out.push_str("=======");
            out.push_str(nl);
            side(&mut out, &theirs_text);
            out.push_str(">>>>>>> theirs");
            out.push_str(nl);
        }
        pos = end;
    }
    out.push_str(&b[pos..].concat());
    TextMerge { text: out, clean }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_in_different_places_merge() {
        let base = "a\nb\nc\nd\ne\n";
        let ours = "A\nb\nc\nd\ne\n";
        let theirs = "a\nb\nc\nd\nE\nf\n";
        let m = merge_lines(base, ours, theirs);
        assert!(m.clean);
        assert_eq!(m.text, "A\nb\nc\nd\nE\nf\n");
    }

    #[test]
    fn the_same_change_on_both_sides_is_clean() {
        let m = merge_lines("a\nb\n", "a\nX\n", "a\nX\n");
        assert!(m.clean);
        assert_eq!(m.text, "a\nX\n");
    }

    #[test]
    fn overlapping_changes_conflict_with_markers() {
        let m = merge_lines("a\nb\nc\n", "a\nours\nc\n", "a\ntheirs\nc\n");
        assert!(!m.clean);
        assert_eq!(
            m.text,
            "a\n<<<<<<< ours\nours\n=======\ntheirs\n>>>>>>> theirs\nc\n"
        );
    }

    #[test]
    fn deletions_and_insertions_at_the_ends() {
        let m = merge_lines("a\nb\nc", "b\nc", "a\nb\nc\nd\n");
        assert!(m.clean, "{}", m.text);
        assert_eq!(m.text, "b\nc\nd\n");
        let m = merge_lines("", "x\n", "y\n");
        assert!(!m.clean);
    }
}
