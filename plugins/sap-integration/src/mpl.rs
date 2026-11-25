// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Reading the message processing log (MPL) of an Integration Suite tenant: the entries of
//! `/api/v1/MessageProcessingLogs` as OData V2 JSON (`{"d": {...}}` or `{"d": {"results":
//! [...]}}`), with timestamps as `/Date(ms)/`.

use serde_json::Value;

/// The header the runtime puts on every answer of an iFlow endpoint.
pub const LOG_ID_HEADER: &str = "SAP_MessageProcessingLogID";

/// What the plugin needs from one log entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub guid: String,
    pub correlation_id: String,
    pub flow: String,
    pub status: String,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub sender: String,
    pub receiver: String,
}

impl Entry {
    pub fn duration_ms(&self) -> Option<i64> {
        Some(self.end_ms? - self.start_ms?).filter(|d| *d >= 0)
    }

    pub fn kind(&self) -> Kind {
        Kind::of(&self.status)
    }
}

/// What a status means for a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Done,
    Running,
    Failed,
    Other,
}

impl Kind {
    pub fn of(status: &str) -> Self {
        match status {
            "COMPLETED" => Kind::Done,
            "PROCESSING" | "RETRY" => Kind::Running,
            "FAILED" | "ESCALATED" | "ABANDONED" => Kind::Failed,
            _ => Kind::Other,
        }
    }
}

/// The entries in `text`; `None` when it is not a message processing log answer.
pub fn parse(text: &str) -> Option<Vec<Entry>> {
    let value: Value = serde_json::from_str(text).ok()?;
    let d = value.get("d").unwrap_or(&value);
    let items: Vec<&Value> = if let Some(list) = d.get("results").and_then(Value::as_array) {
        list.iter().collect()
    } else if d.get("MessageGuid").is_some() {
        vec![d]
    } else {
        return None;
    };
    let entries: Vec<Entry> = items.into_iter().filter_map(entry).collect();
    // An empty list is still a log answer when it has the `results` shape.
    if entries.is_empty() && d.get("results").is_none() {
        return None;
    }
    Some(entries)
}

fn entry(v: &Value) -> Option<Entry> {
    let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
    let guid = text("MessageGuid");
    if guid.is_empty() {
        return None;
    }
    let date = |k: &str| v.get(k).and_then(Value::as_str).and_then(parse_date);
    Some(Entry {
        guid,
        correlation_id: text("CorrelationId"),
        flow: text("IntegrationFlowName"),
        status: text("Status"),
        start_ms: date("LogStart"),
        end_ms: date("LogEnd"),
        sender: text("Sender"),
        receiver: text("Receiver"),
    })
}

/// `/Date(1700000000000)/`, also with an offset (`/Date(1700000000000+0100)/`).
pub fn parse_date(text: &str) -> Option<i64> {
    let inner = text.strip_prefix("/Date(")?.strip_suffix(")/")?;
    let end = inner
        .char_indices()
        .skip(1)
        .find(|(_, c)| matches!(c, '+' | '-'))
        .map_or(inner.len(), |(i, _)| i);
    inner[..end].parse().ok()
}

/// `2.5 s`, `340 ms`, `1 min 5 s`.
pub fn duration(ms: i64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        let tenths = (ms + 50) / 100;
        if tenths % 10 == 0 {
            format!("{} s", tenths / 10)
        } else {
            format!("{}.{} s", tenths / 10, tenths % 10)
        }
    } else {
        format!("{} min {} s", ms / 60_000, (ms % 60_000) / 1000)
    }
}

/// `2026-11-24 10:00:02 UTC`.
pub fn time(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_durations() {
        assert_eq!(parse_date("/Date(1764000000000)/"), Some(1_764_000_000_000));
        assert_eq!(
            parse_date("/Date(1764000000000+0100)/"),
            Some(1_764_000_000_000)
        );
        assert_eq!(parse_date("2026-01-01"), None);
        assert_eq!(time(1_764_000_000_000), "2025-11-24 16:00:00 UTC");
        assert_eq!(duration(340), "340 ms");
        assert_eq!(duration(2500), "2.5 s");
        assert_eq!(duration(3000), "3 s");
        assert_eq!(duration(65_000), "1 min 5 s");
    }

    #[test]
    fn entries_in_both_shapes() {
        let one = r#"{"d":{"MessageGuid":"A1","Status":"FAILED","LogStart":"/Date(1000)/","LogEnd":"/Date(3500)/","IntegrationFlowName":"F"}}"#;
        let e = parse(one).unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].duration_ms(), Some(2500));
        assert_eq!(e[0].kind(), Kind::Failed);
        let many = r#"{"d":{"results":[{"MessageGuid":"A1","Status":"COMPLETED"},{"MessageGuid":"A2","Status":"PROCESSING"}]}}"#;
        assert_eq!(parse(many).unwrap().len(), 2);
        assert_eq!(parse(r#"{"d":{"results":[]}}"#).unwrap().len(), 0);
        assert!(parse(r#"{"hello":1}"#).is_none());
        assert!(parse("not json").is_none());
    }
}
