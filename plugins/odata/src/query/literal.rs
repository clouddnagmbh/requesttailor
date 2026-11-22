// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! EDM literals in `$filter`, per OData version, and the filter builder's expressions.
//!
//! | Type | V2 | V4 |
//! | --- | --- | --- |
//! | `Edm.String` | `'O''Neil'` | `'O''Neil'` |
//! | `Edm.Guid` | `guid'…'` | bare |
//! | `Edm.DateTime` (V2) | `datetime'2026-01-01T00:00:00'` | as `Edm.DateTimeOffset` |
//! | `Edm.DateTimeOffset` | `datetimeoffset'2026-01-01T00:00:00Z'` | `2026-01-01T00:00:00Z` |
//! | `Edm.Date` (V4) | `datetime'2026-01-01T00:00:00'` | `2026-01-01` |
//! | `Edm.Time` (V2) / `Edm.TimeOfDay` (V4) | `time'PT12H30M00S'` | `12:30:00` |
//! | `Edm.Decimal` | `12.5M` | `12.5` |
//! | `Edm.Double` / `Edm.Single` / `Edm.Int64` | `1.5d` / `1.5f` / `10L` | bare |
//! | `Edm.Boolean`, other integers | `true`, `10` | `true`, `10` |
//! | enum types (V4) | — | `Ns.Color'Red'` |
//!
//! `null` is the null literal for every type. A value with a `{{template}}` is not checked;
//! it gets the type's quotes or suffix and is resolved later by the host.

use super::encode::template_ranges;
use crate::edm::Version;

/// The literal for `value` of type `edm_type` in a V2 or V4 `$filter`, or why it is not one.
pub fn literal(edm_type: &str, value: &str, version: Version) -> Result<String, String> {
    let v2 = version == Version::V2;
    let raw = value.trim();
    if raw == "null" {
        return Ok("null".into());
    }
    let template = !template_ranges(raw).is_empty();
    let check = |ok: bool, what: &str, example: &str| {
        if ok || template {
            Ok(())
        } else {
            Err(format!(
                "'{raw}' is not {what} ({edm_type}), e.g. {example}"
            ))
        }
    };
    let ty = edm_type.trim();
    Ok(match ty {
        "Edm.String" => quote(value),
        "Edm.Guid" => {
            check(
                is_guid(raw),
                "a GUID",
                "00000000-0000-0000-0000-000000000001",
            )?;
            if v2 {
                format!("guid'{raw}'")
            } else {
                raw.to_owned()
            }
        }
        "Edm.Boolean" => {
            let lower = raw.to_ascii_lowercase();
            check(lower == "true" || lower == "false", "true or false", "true")?;
            if template { raw.to_owned() } else { lower }
        }
        "Edm.Byte" | "Edm.SByte" | "Edm.Int16" | "Edm.Int32" => {
            check(is_integer(raw), "a whole number", "10")?;
            raw.to_owned()
        }
        "Edm.Int64" => {
            check(is_integer(raw), "a whole number", "10")?;
            if v2 {
                format!("{raw}L")
            } else {
                raw.to_owned()
            }
        }
        "Edm.Decimal" => {
            check(is_decimal(raw), "a decimal number", "12.5")?;
            if v2 {
                format!("{raw}M")
            } else {
                raw.to_owned()
            }
        }
        "Edm.Double" | "Edm.Single" => {
            check(is_float(raw), "a number", "1.5")?;
            match (v2, ty) {
                (true, "Edm.Double") => format!("{raw}d"),
                (true, _) => format!("{raw}f"),
                _ => raw.to_owned(),
            }
        }
        "Edm.DateTime" if v2 => {
            let t = date_time(raw, false, template).ok_or_else(|| {
                format!("'{raw}' is not a date and time ({ty}), e.g. 2026-01-01T00:00:00")
            })?;
            format!("datetime'{t}'")
        }
        "Edm.DateTime" | "Edm.DateTimeOffset" => {
            let t = date_time(raw, true, template).ok_or_else(|| {
                format!("'{raw}' is not a date and time ({ty}), e.g. 2026-01-01T00:00:00Z")
            })?;
            if v2 {
                format!("datetimeoffset'{t}'")
            } else {
                t
            }
        }
        "Edm.Date" => {
            check(is_date(raw), "a date", "2026-01-01")?;
            if v2 {
                format!("datetime'{raw}T00:00:00'")
            } else {
                raw.to_owned()
            }
        }
        "Edm.Time" | "Edm.TimeOfDay" if v2 && raw.starts_with("PT") => format!("time'{raw}'"),
        "Edm.Time" | "Edm.TimeOfDay" => {
            let t = time(raw, template)
                .ok_or_else(|| format!("'{raw}' is not a time ({ty}), e.g. 12:30:00"))?;
            if v2 {
                format!("time'{}'", duration_of(&t, raw))
            } else {
                t
            }
        }
        "Edm.Duration" => format!("duration'{raw}'"),
        "Edm.Binary" => format!("binary'{raw}'"),
        _ if !ty.starts_with("Edm.") && !ty.starts_with("Collection(") && !v2 => {
            // V4 enum type: `Namespace.Type'Member'`.
            format!("{ty}{}", quote(value))
        }
        _ if v2 => quote(value),
        _ => raw.to_owned(),
    })
}

/// A string literal: `'…'` with `'` doubled (outside templates).
pub fn quote(value: &str) -> String {
    let ranges = template_ranges(value);
    let mut out = String::from("'");
    for (i, c) in value.char_indices() {
        out.push(c);
        if c == '\'' && !ranges.iter().any(|&(s, e)| s <= i && i < e) {
            out.push('\'');
        }
    }
    out.push('\'');
    out
}

fn is_integer(s: &str) -> bool {
    let digits = s.strip_prefix(['-', '+']).unwrap_or(s);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

fn is_decimal(s: &str) -> bool {
    let unsigned = s.strip_prefix(['-', '+']).unwrap_or(s);
    match unsigned.split_once('.') {
        Some((a, b)) => {
            !(a.is_empty() && b.is_empty())
                && a.bytes().all(|c| c.is_ascii_digit())
                && b.bytes().all(|c| c.is_ascii_digit())
        }
        None => is_integer(unsigned),
    }
}

fn is_float(s: &str) -> bool {
    if matches!(s, "INF" | "-INF" | "NaN") {
        return true;
    }
    match s.split_once(['e', 'E']) {
        Some((m, e)) => is_decimal(m) && is_integer(e),
        None => is_decimal(s),
    }
}

fn is_guid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && parts
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(p, n)| p.len() == n && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn digits(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_date(s: &str) -> bool {
    let p: Vec<&str> = s.split('-').collect();
    p.len() == 3 && digits(p[0], 4) && digits(p[1], 2) && digits(p[2], 2)
}

/// `HH:MM`, `HH:MM:SS` or `HH:MM:SS.fff`, completed to at least seconds.
fn time(s: &str, template: bool) -> Option<String> {
    if template {
        return Some(s.to_owned());
    }
    let (hms, fraction) = s.split_once('.').unwrap_or((s, ""));
    if !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let p: Vec<&str> = hms.split(':').collect();
    let ok = (p.len() == 2 || p.len() == 3) && p.iter().all(|x| digits(x, 2));
    if !ok {
        return None;
    }
    let mut out = if p.len() == 2 {
        format!("{hms}:00")
    } else {
        hms.to_owned()
    };
    if !fraction.is_empty() {
        out.push('.');
        out.push_str(fraction);
    }
    Some(out)
}

/// `12:30:00` as an XML duration `PT12H30M00S` (V2 `Edm.Time`); templates stay.
fn duration_of(t: &str, raw: &str) -> String {
    if !template_ranges(raw).is_empty() {
        return raw.to_owned();
    }
    let p: Vec<&str> = t.split(':').collect();
    format!("PT{}H{}M{}S", p[0], p[1], p[2])
}

/// A date (completed with `T00:00:00`) or a date and time; with `offset`, a missing offset
/// becomes `Z`; without, a trailing `Z` is dropped (V2 `Edm.DateTime` has none).
fn date_time(s: &str, offset: bool, template: bool) -> Option<String> {
    if template {
        return Some(s.to_owned());
    }
    let (date, rest) = s.split_once('T').unwrap_or((s, ""));
    if !is_date(date) {
        return None;
    }
    let rest = if rest.is_empty() { "00:00:00" } else { rest };
    let split = rest.find(['Z', '+', '-']).unwrap_or(rest.len());
    let (clock, zone) = rest.split_at(split);
    let clock = time(clock, false)?;
    let zone_ok = zone.is_empty()
        || zone == "Z"
        || zone.len() == 6 && {
            let (h, m) = zone[1..].split_once(':').unwrap_or(("", ""));
            digits(h, 2) && digits(m, 2)
        };
    if !zone_ok {
        return None;
    }
    Some(match (offset, zone) {
        (true, "") => format!("{date}T{clock}Z"),
        (true, z) => format!("{date}T{clock}{z}"),
        (false, _) => format!("{date}T{clock}"),
    })
}

// -------------------------------------------------------------------------------------------
// The filter builder.

/// Comparison and string operators of the filter builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Contains,
    StartsWith,
    EndsWith,
}

impl Operator {
    pub const ALL: [Operator; 9] = [
        Operator::Eq,
        Operator::Ne,
        Operator::Gt,
        Operator::Ge,
        Operator::Lt,
        Operator::Le,
        Operator::Contains,
        Operator::StartsWith,
        Operator::EndsWith,
    ];

    /// As shown in the form: `eq`, …, `contains`, `startswith`, `endswith`.
    pub fn as_str(self) -> &'static str {
        match self {
            Operator::Eq => "eq",
            Operator::Ne => "ne",
            Operator::Gt => "gt",
            Operator::Ge => "ge",
            Operator::Lt => "lt",
            Operator::Le => "le",
            Operator::Contains => "contains",
            Operator::StartsWith => "startswith",
            Operator::EndsWith => "endswith",
        }
    }

    pub fn parse(s: &str) -> Option<Operator> {
        Operator::ALL.into_iter().find(|o| o.as_str() == s.trim())
    }
}

/// One row of the filter builder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    pub property: String,
    /// The property's EDM type (`Edm.String` when unknown).
    pub edm_type: String,
    pub operator: Operator,
    pub value: String,
}

/// One condition: `Name eq 'x'`; `contains` is `substringof('x',Name)` in V2 and
/// `contains(Name,'x')` in V4; `startswith`/`endswith` are functions in both.
pub fn condition(c: &Condition, version: Version) -> Result<String, String> {
    let lit =
        literal(&c.edm_type, &c.value, version).map_err(|e| format!("{}: {e}", c.property))?;
    let p = &c.property;
    Ok(match (c.operator, version) {
        (Operator::Contains, Version::V2) => format!("substringof({lit},{p})"),
        (Operator::Contains, Version::V4) => format!("contains({p},{lit})"),
        (Operator::StartsWith, _) => format!("startswith({p},{lit})"),
        (Operator::EndsWith, _) => format!("endswith({p},{lit})"),
        (op, _) => format!("{p} {} {lit}", op.as_str()),
    })
}

/// The conditions joined with `and` (or `or` when `any`).
pub fn filter(conditions: &[Condition], any: bool, version: Version) -> Result<String, String> {
    let parts = conditions
        .iter()
        .map(|c| condition(c, version))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(parts.join(if any { " or " } else { " and " }))
}

/// An example condition for a property of `edm_type`, for the help table.
pub fn example(property: &str, edm_type: &str, version: Version) -> String {
    let (operator, value) = match edm_type {
        "Edm.String" => (Operator::Eq, "x"),
        "Edm.Guid" => (Operator::Eq, "00000000-0000-0000-0000-000000000001"),
        "Edm.Boolean" => (Operator::Eq, "true"),
        "Edm.Byte" | "Edm.SByte" | "Edm.Int16" | "Edm.Int32" | "Edm.Int64" => (Operator::Gt, "10"),
        "Edm.Decimal" => (Operator::Gt, "12.5"),
        "Edm.Double" | "Edm.Single" => (Operator::Gt, "1.5"),
        "Edm.DateTime" => (Operator::Ge, "2026-01-01T00:00:00"),
        "Edm.DateTimeOffset" => (Operator::Ge, "2026-01-01T00:00:00Z"),
        "Edm.Date" => (Operator::Ge, "2026-01-01"),
        "Edm.Time" | "Edm.TimeOfDay" => (Operator::Ge, "12:00:00"),
        _ => (Operator::Eq, "x"),
    };
    let c = |operator, value: &str| Condition {
        property: property.to_owned(),
        edm_type: edm_type.to_owned(),
        operator,
        value: value.to_owned(),
    };
    let first = condition(&c(operator, value), version).unwrap_or_default();
    if edm_type == "Edm.String" {
        let second = condition(&c(Operator::Contains, "x"), version).unwrap_or_default();
        format!("{first}  ·  {second}")
    } else {
        first
    }
}
