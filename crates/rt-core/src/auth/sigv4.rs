// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! AWS Signature Version 4 (wave 5, strand D): signs the final request (method, URL,
//! headers, body hash) as the last step before the wire. Header signing only (no presigned
//! URLs), `AWS4-HMAC-SHA256`.
//!
//! Canonical request, following the AWS documentation and the CRT test suite
//! (`testdata/sigv4`):
//!
//! - **Path**: the path as it goes on the wire (already percent-encoded). For every service
//!   but S3 it is normalized (dot segments and empty segments removed) and encoded a second
//!   time (`%` becomes `%25`); for S3 it is left as it is, only bytes outside the unreserved
//!   set that are not escapes yet are encoded.
//! - **Query**: every parameter decoded and encoded again with the unreserved set (`+` stays a
//!   literal plus, `%2B`), sorted by name, then value; `name` without `=` signs as `name=`.
//! - **Headers**: `host` from the URL's authority plus every request header except
//!   `authorization`, `user-agent`, `expect`, `x-amzn-trace-id`, `connection` and
//!   `transfer-encoding`; names lowercased, values trimmed with inner whitespace runs
//!   collapsed, repeated headers joined with `,` in order. The body's content type is added as
//!   `Content-Type` first if the request has none (the transport would add it unsigned).
//! - **Payload**: SHA-256 of the body, or the value of an `X-Amz-Content-Sha256` header the
//!   request already has (e.g. `UNSIGNED-PAYLOAD`). S3 gets that header added.
//!
//! `profile_name` is only read from the file: without an access key and a secret key there is
//! nothing to sign with, and sending fails with a message that says so. RequestTailor does
//! not read `~/.aws` (the same request must sign identically in the browser).

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::model::AwsSigV4;
use crate::transport::HttpRequest;
use crate::vars::Scope;

const ALGORITHM: &str = "AWS4-HMAC-SHA256";

/// Headers that proxies or the transport may change; they are never signed.
const UNSIGNED_HEADERS: &[&str] = &[
    "authorization",
    "user-agent",
    "expect",
    "x-amzn-trace-id",
    "connection",
    "transfer-encoding",
];

/// Resolved credentials and scope of one signature.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
    pub region: String,
    pub service: String,
}

impl std::fmt::Debug for Credentials {
    // The secret key and the session token never reach logs or messages.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("access_key_id", &self.access_key_id)
            .field("region", &self.region)
            .field("service", &self.service)
            .finish_non_exhaustive()
    }
}

/// The parts of the algorithm that differ between services.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignOptions {
    /// Remove dot segments and empty segments from the path (not for S3).
    pub normalize_path: bool,
    /// Encode the (already encoded) path once more (not for S3).
    pub double_encode_path: bool,
    /// Add `X-Amz-Content-Sha256` (required by S3).
    pub content_sha256_header: bool,
    /// Sign `X-Amz-Security-Token`; `false` adds it after signing (a few services want that).
    pub sign_session_token: bool,
}

impl SignOptions {
    /// What AWS expects from `service`: S3 is the exception for path handling and the
    /// payload header.
    pub fn for_service(service: &str) -> Self {
        let s3 = service.eq_ignore_ascii_case("s3");
        Self {
            normalize_path: !s3,
            double_encode_path: !s3,
            content_sha256_header: s3,
            sign_session_token: true,
        }
    }
}

/// The intermediate results, for tests and diagnostics. Contains no secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub canonical_request: String,
    pub string_to_sign: String,
    /// Lowercase hex.
    pub signature: String,
}

/// Adds `Authorization`, `X-Amz-Date` (and `X-Amz-Security-Token`, `X-Amz-Content-Sha256`
/// where needed) to `request`. Templates in `auth` are resolved against `scope`. Returns the
/// resolved secret inputs (secret key, session token) to mask in the report.
pub fn sign(
    request: &mut HttpRequest,
    auth: &AwsSigV4,
    scope: &Scope,
    now: web_time::SystemTime,
) -> Result<Vec<String>, String> {
    let credentials = resolve(auth, scope)?;
    let secrets = std::iter::once(credentials.secret_access_key.clone())
        .chain(credentials.session_token.clone())
        .collect();
    let options = SignOptions::for_service(&credentials.service);
    sign_request(request, &credentials, &options, now)?;
    Ok(secrets)
}

/// Resolves the templates of `auth` and checks that it can sign.
fn resolve(auth: &AwsSigV4, scope: &Scope) -> Result<Credentials, String> {
    let field = |label: &str, template: &str| -> Result<String, String> {
        let resolved = scope.interpolate(template);
        match resolved.unresolved.first() {
            Some(name) => Err(format!(
                "AWS Signature V4: the {label} uses the variable `{name}`, which has no value"
            )),
            None => Ok(resolved.value.trim().to_owned()),
        }
    };
    let access_key_id = field("access key ID", &auth.access_key_id)?;
    let secret_access_key = field("secret access key", &auth.secret_access_key)?;
    let session_token = field("session token", &auth.session_token)?;
    let region = field("region", &auth.region)?;
    let service = field("service", &auth.service)?;
    let profile = field("profile name", &auth.profile_name)?;
    if access_key_id.is_empty() || secret_access_key.is_empty() {
        return Err(if profile.is_empty() {
            "AWS Signature V4: an access key ID and a secret access key are required".to_owned()
        } else {
            format!(
                "AWS Signature V4: the request names the AWS profile `{profile}`, but \
                 RequestTailor does not read AWS profiles; set the access key ID and the \
                 secret access key (as secret variables) to sign"
            )
        });
    }
    if region.is_empty() {
        return Err("AWS Signature V4: the region is missing".to_owned());
    }
    if service.is_empty() {
        return Err("AWS Signature V4: the service is missing".to_owned());
    }
    Ok(Credentials {
        access_key_id,
        secret_access_key,
        session_token: (!session_token.is_empty()).then_some(session_token),
        region,
        service,
    })
}

/// Signs `request` in place with resolved credentials. See the module docs for the rules.
pub fn sign_request(
    request: &mut HttpRequest,
    credentials: &Credentials,
    options: &SignOptions,
    now: web_time::SystemTime,
) -> Result<Signature, String> {
    let url = UrlParts::parse(&request.url)?;
    let secs = now
        .duration_since(web_time::UNIX_EPOCH)
        .map_err(|_| "AWS Signature V4: the system clock is before 1970".to_owned())?
        .as_secs();
    let (date, date_time) = amz_date(secs);

    // The transport adds the body's content type when the request has none; sign it.
    if let Some(content_type) = request.body.as_ref().and_then(|b| b.content_type.clone())
        && !has_header(&request.headers, "content-type")
    {
        request
            .headers
            .push(("Content-Type".to_owned(), content_type));
    }
    let payload_hash = match header_value(&request.headers, "x-amz-content-sha256") {
        Some(given) => given.trim().to_owned(),
        None => hex(&Sha256::digest(
            request.body.as_ref().map_or(&b""[..], |b| &b.bytes[..]),
        )),
    };
    let replaced = |name: &str| {
        name.eq_ignore_ascii_case("authorization")
            || name.eq_ignore_ascii_case("x-amz-date")
            || (credentials.session_token.is_some()
                && name.eq_ignore_ascii_case("x-amz-security-token"))
    };
    request.headers.retain(|(name, _)| !replaced(name));
    request
        .headers
        .push(("X-Amz-Date".to_owned(), date_time.clone()));
    if options.content_sha256_header && !has_header(&request.headers, "x-amz-content-sha256") {
        request
            .headers
            .push(("X-Amz-Content-Sha256".to_owned(), payload_hash.clone()));
    }
    if let Some(token) = &credentials.session_token
        && options.sign_session_token
    {
        request
            .headers
            .push(("X-Amz-Security-Token".to_owned(), token.clone()));
    }

    let (canonical_headers, signed_headers) = canonical_headers(&url.host, &request.headers);
    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        request.method.to_ascii_uppercase(),
        canonical_path(url.path, options),
        canonical_query(url.query),
        canonical_headers,
        signed_headers,
        payload_hash
    );
    let credential_scope = format!(
        "{date}/{}/{}/aws4_request",
        credentials.region, credentials.service
    );
    let string_to_sign = format!(
        "{ALGORITHM}\n{date_time}\n{credential_scope}\n{}",
        hex(&Sha256::digest(canonical_request.as_bytes()))
    );
    let key = signing_key(credentials, &date);
    let signature = hex(&hmac(&key, string_to_sign.as_bytes()));

    if let Some(token) = &credentials.session_token
        && !options.sign_session_token
    {
        request
            .headers
            .push(("X-Amz-Security-Token".to_owned(), token.clone()));
    }
    request.headers.push((
        "Authorization".to_owned(),
        format!(
            "{ALGORITHM} Credential={}/{credential_scope}, SignedHeaders={signed_headers}, \
             Signature={signature}",
            credentials.access_key_id
        ),
    ));
    Ok(Signature {
        canonical_request,
        string_to_sign,
        signature,
    })
}

// ---------------------------------------------------------------------------------------------
// Pieces

/// Host (with port, as written), path and query of an absolute URL, without parsing it into
/// a normalized form: the signature covers exactly what the transport sends.
struct UrlParts<'a> {
    host: String,
    path: &'a str,
    query: &'a str,
}

impl<'a> UrlParts<'a> {
    fn parse(url: &'a str) -> Result<Self, String> {
        let invalid = || format!("AWS Signature V4: cannot sign the URL {url}");
        let (_, rest) = url.split_once("://").ok_or_else(invalid)?;
        let rest = rest.split('#').next().unwrap_or("");
        let end = rest.find(['/', '?']).unwrap_or(rest.len());
        let authority = &rest[..end];
        let authority = authority.rsplit('@').next().unwrap_or(authority);
        if authority.is_empty() {
            return Err(invalid());
        }
        let rest = &rest[end..];
        let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
        Ok(Self {
            host: authority.to_ascii_lowercase(),
            path,
            query,
        })
    }
}

fn has_header(headers: &[(String, String)], name: &str) -> bool {
    headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))
}

fn header_value<'h>(headers: &'h [(String, String)], name: &str) -> Option<&'h str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// `(canonical headers block, signed header names)`.
fn canonical_headers(host: &str, headers: &[(String, String)]) -> (String, String) {
    let mut entries: Vec<(String, Vec<String>)> = vec![("host".to_owned(), vec![host.to_owned()])];
    for (name, value) in headers {
        let name = name.trim().to_ascii_lowercase();
        if name == "host" || UNSIGNED_HEADERS.contains(&name.as_str()) {
            continue;
        }
        let value = collapse_whitespace(value);
        match entries.iter_mut().find(|(n, _)| *n == name) {
            Some((_, values)) => values.push(value),
            None => entries.push((name, vec![value])),
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let block: String = entries
        .iter()
        .map(|(name, values)| format!("{name}:{}\n", values.join(",")))
        .collect();
    let signed = entries
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(";");
    (block, signed)
}

/// Trims and replaces every run of whitespace (including folded lines) with one space.
fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn canonical_path(path: &str, options: &SignOptions) -> String {
    let path = if path.is_empty() { "/" } else { path };
    let normalized;
    let path = if options.normalize_path {
        normalized = normalize_path(path);
        normalized.as_str()
    } else {
        path
    };
    if options.double_encode_path {
        encode(path.as_bytes(), true)
    } else {
        encode_unescaped(path)
    }
}

/// RFC 3986 5.2.4 dot-segment removal, also dropping empty segments, keeping a trailing
/// slash (as the AWS SDKs do).
fn normalize_path(path: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            s => segments.push(s),
        }
    }
    let mut out = format!("/{}", segments.join("/"));
    let trailing = path.ends_with('/') || path.ends_with("/.") || path.ends_with("/..");
    if trailing && !out.ends_with('/') {
        out.push('/');
    }
    out
}

fn canonical_query(query: &str) -> String {
    let mut params: Vec<(String, String)> = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (name, value) = p.split_once('=').unwrap_or((p, ""));
            (
                encode(&percent_decode(name), false),
                encode(&percent_decode(value), false),
            )
        })
        .collect();
    params.sort();
    params
        .iter()
        .map(|(n, v)| format!("{n}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
}

/// AWS URI encoding: everything but unreserved bytes (and `/` if `keep_slash`) as `%XX`.
fn encode(bytes: &[u8], keep_slash: bool) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if is_unreserved(b) || (keep_slash && b == b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Single encoding of a path that is already encoded: existing `%XX` escapes stay (in upper
/// case), every other byte outside the unreserved set and `/` is encoded.
fn encode_unescaped(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            out.push('%');
            out.push(bytes[i + 1].to_ascii_uppercase() as char);
            out.push(bytes[i + 2].to_ascii_uppercase() as char);
            i += 3;
            continue;
        }
        if is_unreserved(b) || b == b'/' {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
        i += 1;
    }
    out
}

/// Decodes `%XX` escapes; anything else (including `+`) stays as it is.
fn percent_decode(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2]))
        {
            out.push(h << 4 | l);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

fn hex_digit(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes keys of any length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn signing_key(credentials: &Credentials, date: &str) -> Vec<u8> {
    let k_secret = format!("AWS4{}", credentials.secret_access_key);
    let k_date = hmac(k_secret.as_bytes(), date.as_bytes());
    let k_region = hmac(&k_date, credentials.region.as_bytes());
    let k_service = hmac(&k_region, credentials.service.as_bytes());
    hmac(&k_service, b"aws4_request")
}

/// `(YYYYMMDD, YYYYMMDDTHHMMSSZ)` in UTC.
fn amz_date(secs: u64) -> (String, String) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    let date = format!("{y:04}{m:02}{d:02}");
    let date_time = format!(
        "{date}T{:02}{:02}{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    );
    (date, date_time)
}

/// Days since 1970-01-01 → proleptic Gregorian date (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vars::{VarLayer, VarSource};

    fn request(url: &str) -> HttpRequest {
        HttpRequest {
            method: "GET".into(),
            url: url.into(),
            headers: Vec::new(),
            body: None,
            options: Default::default(),
        }
    }

    fn scope(vars: &[(&str, &str, bool)]) -> Scope {
        Scope {
            layers: vec![VarLayer {
                source: VarSource::Runtime,
                vars: vars
                    .iter()
                    .map(|(n, v, s)| ((*n).to_owned(), (*v).to_owned(), *s))
                    .collect(),
            }],
        }
    }

    fn at(secs: u64) -> web_time::SystemTime {
        web_time::UNIX_EPOCH + std::time::Duration::from_secs(secs)
    }

    #[test]
    fn dates() {
        assert_eq!(amz_date(0), ("19700101".into(), "19700101T000000Z".into()));
        // 2015-08-30T12:36:00Z
        assert_eq!(amz_date(1_440_938_160).1, "20150830T123600Z");
        // 2024-02-29T23:59:59Z (leap day)
        assert_eq!(amz_date(1_709_251_199).1, "20240229T235959Z");
    }

    #[test]
    fn templates_are_resolved_and_secrets_returned() {
        let auth = AwsSigV4 {
            access_key_id: "{{key}}".into(),
            secret_access_key: "{{secret}}".into(),
            session_token: "{{token}}".into(),
            service: "execute-api".into(),
            region: "{{region}}".into(),
            profile_name: String::new(),
        };
        let scope = scope(&[
            ("key", "AKIDEXAMPLE", false),
            ("secret", "very-secret", true),
            ("token", "tok-123", true),
            ("region", "eu-central-1", false),
        ]);
        let mut req = request("https://abc.execute-api.eu-central-1.amazonaws.com/prod/x?b=2&a=1");
        let secrets = sign(&mut req, &auth, &scope, at(1_440_938_160)).unwrap();
        assert_eq!(secrets, ["very-secret", "tok-123"]);
        let authz = header_value(&req.headers, "authorization").unwrap();
        assert!(
            authz.starts_with(
                "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/eu-central-1/execute-api/aws4_request, SignedHeaders=host;x-amz-date;x-amz-security-token, Signature="
            ),
            "{authz}"
        );
        assert!(!authz.contains("very-secret"));
        assert_eq!(
            header_value(&req.headers, "x-amz-security-token"),
            Some("tok-123")
        );
    }

    #[test]
    fn s3_adds_the_payload_header_and_keeps_the_path() {
        let auth = AwsSigV4 {
            access_key_id: "AKID".into(),
            secret_access_key: "s".into(),
            service: "s3".into(),
            region: "us-east-1".into(),
            ..Default::default()
        };
        let mut req = request("https://bucket.s3.amazonaws.com//a/./b%20c");
        sign(&mut req, &auth, &Scope::default(), at(0)).unwrap();
        assert_eq!(
            header_value(&req.headers, "x-amz-content-sha256"),
            Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
        );
        let opts = SignOptions::for_service("s3");
        assert_eq!(canonical_path("//a/./b%20c", &opts), "//a/./b%20c");
        let opts = SignOptions::for_service("lambda");
        assert_eq!(canonical_path("//a/./b%20c", &opts), "/a/b%2520c");
    }

    #[test]
    fn an_existing_payload_header_is_the_payload_hash() {
        let creds = Credentials {
            access_key_id: "AKID".into(),
            secret_access_key: "s".into(),
            session_token: None,
            region: "us-east-1".into(),
            service: "s3".into(),
        };
        let mut req = request("https://b.s3.amazonaws.com/k");
        req.headers
            .push(("x-amz-content-sha256".into(), "UNSIGNED-PAYLOAD".into()));
        let sig = sign_request(&mut req, &creds, &SignOptions::for_service("s3"), at(0)).unwrap();
        assert!(sig.canonical_request.ends_with("\nUNSIGNED-PAYLOAD"));
        let count = req
            .headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case("x-amz-content-sha256"))
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn re_signing_replaces_the_old_signature() {
        let creds = Credentials {
            access_key_id: "AKID".into(),
            secret_access_key: "s".into(),
            session_token: None,
            region: "us-east-1".into(),
            service: "sqs".into(),
        };
        let mut req = request("https://sqs.us-east-1.amazonaws.com/");
        req.headers
            .push(("Authorization".into(), "Bearer old".into()));
        let opts = SignOptions::for_service("sqs");
        let first = sign_request(&mut req, &creds, &opts, at(0)).unwrap();
        let second = sign_request(&mut req, &creds, &opts, at(0)).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            req.headers
                .iter()
                .filter(|(n, _)| n.eq_ignore_ascii_case("authorization"))
                .count(),
            1
        );
    }

    #[test]
    fn content_type_of_the_body_is_signed() {
        let creds = Credentials {
            access_key_id: "AKID".into(),
            secret_access_key: "s".into(),
            session_token: None,
            region: "us-east-1".into(),
            service: "service".into(),
        };
        let mut req = request("https://example.amazonaws.com/");
        req.method = "POST".into();
        req.body = Some(crate::transport::HttpBody {
            content_type: Some("application/json".into()),
            bytes: bytes::Bytes::from_static(b"{}"),
        });
        let sig = sign_request(&mut req, &creds, &SignOptions::for_service("x"), at(0)).unwrap();
        assert!(
            sig.canonical_request
                .contains("\ncontent-type:application/json\n")
        );
        assert!(
            sig.canonical_request
                .contains("content-type;host;x-amz-date")
        );
    }

    #[test]
    fn missing_keys_explain_the_profile() {
        let auth = AwsSigV4 {
            profile_name: "work".into(),
            service: "s3".into(),
            region: "us-east-1".into(),
            ..Default::default()
        };
        let err = sign(&mut request("https://x/"), &auth, &Scope::default(), at(0)).unwrap_err();
        assert!(err.contains("profile `work`"), "{err}");
        assert!(err.contains("does not read AWS profiles"), "{err}");
        let auth = AwsSigV4 {
            service: "s3".into(),
            region: "us-east-1".into(),
            ..Default::default()
        };
        let err = sign(&mut request("https://x/"), &auth, &Scope::default(), at(0)).unwrap_err();
        assert!(
            err.contains("access key ID and a secret access key"),
            "{err}"
        );
    }

    #[test]
    fn missing_region_service_or_variables_fail() {
        let base = AwsSigV4 {
            access_key_id: "AKID".into(),
            secret_access_key: "s".into(),
            service: "s3".into(),
            region: "us-east-1".into(),
            ..Default::default()
        };
        let no_region = AwsSigV4 {
            region: String::new(),
            ..base.clone()
        };
        assert!(
            sign(
                &mut request("https://x/"),
                &no_region,
                &Scope::default(),
                at(0)
            )
            .unwrap_err()
            .contains("region")
        );
        let no_service = AwsSigV4 {
            service: String::new(),
            ..base.clone()
        };
        assert!(
            sign(
                &mut request("https://x/"),
                &no_service,
                &Scope::default(),
                at(0)
            )
            .unwrap_err()
            .contains("service")
        );
        let unresolved = AwsSigV4 {
            secret_access_key: "{{awsSecret}}".into(),
            ..base
        };
        let err = sign(
            &mut request("https://x/"),
            &unresolved,
            &Scope::default(),
            at(0),
        )
        .unwrap_err();
        assert!(err.contains("`awsSecret`"), "{err}");
    }

    #[test]
    fn query_canonicalization() {
        assert_eq!(canonical_query(""), "");
        assert_eq!(canonical_query("b=2&a=1&a=0"), "a=0&a=1&b=2");
        assert_eq!(canonical_query("flag&x="), "flag=&x=");
        assert_eq!(canonical_query("q=a+b%20c&%7e=~"), "q=a%2Bb%20c&~=~");
    }

    #[test]
    fn path_normalization() {
        assert_eq!(normalize_path("/"), "/");
        assert_eq!(normalize_path("//a//b/"), "/a/b/");
        assert_eq!(normalize_path("/a/b/../.."), "/");
        assert_eq!(normalize_path("/a/./b"), "/a/b");
        assert_eq!(normalize_path("/a/b/."), "/a/b/");
    }

    #[test]
    fn credentials_debug_hides_secrets() {
        let creds = Credentials {
            access_key_id: "AKID".into(),
            secret_access_key: "hidden-key".into(),
            session_token: Some("hidden-token".into()),
            region: "r".into(),
            service: "s".into(),
        };
        let debug = format!("{creds:?}");
        assert!(!debug.contains("hidden"), "{debug}");
    }
}
