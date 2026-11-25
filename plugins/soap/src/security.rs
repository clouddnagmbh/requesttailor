// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! WS-Security headers: a UsernameToken whose credentials are `{{variable}}` placeholders
//! (the plugin never sees secret values; the core fills them in when sending) and a Timestamp.
//! Only the clear-text password type is possible: a digest needs the password.
//! Signature and encryption are out of scope.

use crate::envelope::SoapVersion;
use crate::node::escape_text;

pub const WSSE_NS: &str =
    "http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd";
pub const WSU_NS: &str =
    "http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd";
const PASSWORD_TEXT: &str = "http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-username-token-profile-1.0#PasswordText";

/// Default variable names of the credentials.
pub const USER_VAR: &str = "wsse_username";
pub const PASSWORD_VAR: &str = "wsse_password";
/// Default time to live of a timestamp, seconds.
pub const DEFAULT_TTL: u64 = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Security {
    pub username_token: bool,
    pub user_var: String,
    pub password_var: String,
    /// Time to live in seconds; `None`: no timestamp.
    pub timestamp_ttl: Option<u64>,
}

impl Default for Security {
    fn default() -> Self {
        Security {
            username_token: true,
            user_var: USER_VAR.into(),
            password_var: PASSWORD_VAR.into(),
            timestamp_ttl: None,
        }
    }
}

/// The `wsse:Security` element as lines of text (indented relative to itself). `now_ms` is
/// the time the timestamp starts.
pub fn header(version: SoapVersion, prefix: &str, security: &Security, now_ms: u64) -> String {
    let must = match version {
        SoapVersion::V11 => format!("{prefix}:mustUnderstand=\"1\""),
        SoapVersion::V12 => format!("{prefix}:mustUnderstand=\"true\""),
    };
    let mut out =
        format!("<wsse:Security xmlns:wsse=\"{WSSE_NS}\" xmlns:wsu=\"{WSU_NS}\" {must}>\n");
    if let Some(ttl) = security.timestamp_ttl {
        out.push_str(&timestamp_block(now_ms, ttl));
    }
    if security.username_token {
        out.push_str(&format!(
            "  <wsse:UsernameToken wsu:Id=\"UsernameToken-1\">\n    <wsse:Username>{{{{{}}}}}</wsse:Username>\n    <wsse:Password Type=\"{PASSWORD_TEXT}\">{{{{{}}}}}</wsse:Password>\n  </wsse:UsernameToken>\n",
            escape_text(&security.user_var),
            escape_text(&security.password_var)
        ));
    }
    out.push_str("</wsse:Security>");
    out
}

fn timestamp_block(now_ms: u64, ttl: u64) -> String {
    format!(
        "  <wsu:Timestamp wsu:Id=\"TS-1\">\n    <wsu:Created>{}</wsu:Created>\n    <wsu:Expires>{}</wsu:Expires>\n  </wsu:Timestamp>\n",
        iso(now_ms),
        iso(now_ms + ttl * 1000)
    )
}

/// `2026-11-25T10:00:00Z` for Unix milliseconds.
pub fn iso(ms: u64) -> String {
    let secs = ms / 1000;
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Unix milliseconds of a `YYYY-MM-DDTHH:MM:SS[.fff]Z` timestamp.
pub fn parse_iso(text: &str) -> Option<u64> {
    let t = text.trim().trim_end_matches('Z');
    let (date, time) = t.split_once('T')?;
    let mut d = date.split('-');
    let (y, m, day): (i64, i64, i64) = (
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
    );
    let time = time.split('.').next()?;
    let mut tp = time.split(':');
    let (hh, mm, ss): (u64, u64, u64) = (
        tp.next()?.parse().ok()?,
        tp.next()?.parse().ok()?,
        tp.next()?.parse().ok()?,
    );
    let days = days_from_civil(y, m, day);
    if days < 0 {
        return None;
    }
    Some((days as u64 * 86_400 + hh * 3600 + mm * 60 + ss) * 1000)
}

/// Days since 1970-01-01 to (year, month, day) (proleptic Gregorian).
fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Replaces the text of `<…:Created>` and `<…:Expires>` in an envelope text with a timestamp
/// starting at `now_ms`, keeping the time to live the envelope had (or `DEFAULT_TTL`).
/// Returns `None` when the text has no timestamp.
pub fn refresh_timestamp(text: &str, now_ms: u64) -> Option<String> {
    let (c_start, c_end) = element_text_range(text, "Created")?;
    let (e_start, e_end) = element_text_range(text, "Expires")?;
    let created = parse_iso(&text[c_start..c_end]);
    let expires = parse_iso(&text[e_start..e_end]);
    let ttl = match (created, expires) {
        (Some(c), Some(e)) if e > c => (e - c) / 1000,
        _ => DEFAULT_TTL,
    };
    // Replace the later range first so the earlier offsets stay valid.
    let mut ranges = [
        (c_start, c_end, iso(now_ms)),
        (e_start, e_end, iso(now_ms + ttl * 1000)),
    ];
    ranges.sort_by_key(|r| std::cmp::Reverse(r.0));
    let mut out = text.to_owned();
    for (s, e, v) in ranges {
        out.replace_range(s..e, &v);
    }
    Some(out)
}

/// The byte range of the text of the first element with local name `local`.
fn element_text_range(text: &str, local: &str) -> Option<(usize, usize)> {
    let mut from = 0;
    while let Some(i) = text[from..].find('<') {
        let start = from + i;
        let tag_end = start + text[start..].find('>')?;
        let tag = &text[start + 1..tag_end];
        let name = tag.split_whitespace().next().unwrap_or("");
        let name_local = name.rsplit(':').next().unwrap_or(name);
        if !tag.starts_with('/') && !tag.ends_with('/') && name_local == local {
            let content = tag_end + 1;
            let close = content + text[content..].find('<')?;
            return Some((content, close));
        }
        from = tag_end + 1;
    }
    None
}

/// The `Expires` time of the first timestamp of an envelope text, Unix milliseconds.
pub fn expires(text: &str) -> Option<u64> {
    let (s, e) = element_text_range(text, "Expires")?;
    parse_iso(&text[s..e])
}

/// Whether the envelope text carries a UsernameToken, and the placeholder names it uses.
pub fn username_token_vars(text: &str) -> Option<(String, String)> {
    let (us, ue) = element_text_range(text, "Username")?;
    let (ps, pe) = element_text_range(text, "Password")?;
    let var = |s: &str| {
        s.trim()
            .strip_prefix("{{")
            .and_then(|r| r.strip_suffix("}}"))
            .map(str::to_owned)
    };
    Some((
        var(&text[us..ue]).unwrap_or_else(|| USER_VAR.into()),
        var(&text[ps..pe]).unwrap_or_else(|| PASSWORD_VAR.into()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_round_trip() {
        let ms = 1_795_600_000_000;
        assert_eq!(parse_iso(&iso(ms)), Some(ms));
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso(951_782_400_000), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn header_has_placeholders_only() {
        let h = header(
            SoapVersion::V11,
            "soapenv",
            &Security {
                timestamp_ttl: Some(60),
                ..Security::default()
            },
            0,
        );
        assert!(h.contains("{{wsse_username}}") && h.contains("{{wsse_password}}"));
        assert!(h.contains("<wsu:Expires>1970-01-01T00:01:00Z</wsu:Expires>"));
        assert!(h.contains("soapenv:mustUnderstand=\"1\""));
    }

    #[test]
    fn refresh_keeps_the_time_to_live() {
        let h = header(
            SoapVersion::V12,
            "soapenv",
            &Security {
                timestamp_ttl: Some(120),
                ..Security::default()
            },
            1_000_000,
        );
        let fresh = refresh_timestamp(&h, 9_000_000).unwrap();
        assert_eq!(expires(&fresh), Some(9_120_000));
        assert!(refresh_timestamp("<a/>", 0).is_none());
    }

    #[test]
    fn finds_credential_variables() {
        let h = header(
            SoapVersion::V11,
            "soapenv",
            &Security {
                user_var: "u".into(),
                password_var: "p".into(),
                ..Security::default()
            },
            0,
        );
        assert_eq!(username_token_vars(&h), Some(("u".into(), "p".into())));
    }
}
