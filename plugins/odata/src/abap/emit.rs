// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The ABAP text: one writer per flavor over the same [`Call`]. Everything the user must supply
//! (destination, communication scenario, ETag) is a constant or variable with a placeholder in
//! angle brackets; the CSRF token is read at run time.

use super::Flavor;
use super::request::{Call, Kind};

/// Characters per string chunk; a statement line stays below 100 columns.
const CHUNK: usize = 56;

struct Out {
    lines: Vec<String>,
}

impl Out {
    fn new() -> Self {
        Out { lines: Vec::new() }
    }

    fn line(&mut self, indent: usize, text: impl AsRef<str>) {
        let text = text.as_ref();
        if text.is_empty() {
            self.lines.push(String::new());
        } else {
            self.lines.push(format!("{}{text}", "  ".repeat(indent)));
        }
    }

    fn comment(&mut self, indent: usize, text: impl AsRef<str>) {
        self.line(indent, format!("\" {}", text.as_ref()));
    }

    fn finish(self) -> String {
        let mut text = self.lines.join("\n");
        text.push('\n');
        text
    }

    /// `head` and the operands joined by `&&`, one per line, ended by `.`.
    fn assign_parts(&mut self, indent: usize, head: &str, parts: &[String]) {
        let pad = " ".repeat(head.chars().count() + 1);
        let last = parts.len() - 1;
        for (i, part) in parts.iter().enumerate() {
            let tail = if i == last { "." } else { " &&" };
            let lead = if i == 0 {
                format!("{head} ")
            } else {
                pad.clone()
            };
            self.line(indent, format!("{lead}{part}{tail}"));
        }
    }

    /// `head` and a string as backtick literal chunks.
    fn assign_string(&mut self, indent: usize, head: &str, value: &str) {
        let parts: Vec<String> = chunks(value).iter().map(|c| literal(c)).collect();
        self.assign_parts(indent, head, &parts);
    }
}

/// A backtick literal; backticks inside are doubled.
fn literal(text: &str) -> String {
    format!("`{}`", text.replace('`', "``"))
}

/// `text` in pieces of at most [`CHUNK`] characters, cut after a separator when there is one
/// in the last third (at least one piece, so `` stays).
fn chunks(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let mut end = (start + CHUNK).min(chars.len());
        if end < chars.len()
            && let Some(cut) = (start + CHUNK * 2 / 3..end).rev().find(|&i| {
                matches!(
                    chars[i],
                    '&' | '/' | ',' | '?' | ';' | ' ' | '=' | ')' | '('
                )
            })
        {
            end = cut + 1;
        }
        out.push(chars[start..end].iter().collect());
        start = end;
    }
    out
}

/// A comment-safe form of a request name (one line, no quotes).
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}

pub fn write(call: &Call, flavor: Flavor) -> String {
    let mut out = Out::new();
    out.comment(
        0,
        format!("RequestTailor: {}", shorten(&clean(&call.name), 70)),
    );
    out.comment(
        0,
        format!("{} {}", call.method, shorten(&short_uri(call), 80)),
    );
    out.comment(
        0,
        "Generated, not run against any system: review it before you use it.",
    );
    match flavor {
        Flavor::Classic => classic(&mut out, call),
        Flavor::Cloud => cloud(&mut out, call),
    }
    out.finish()
}

/// At most `max` characters, cut with an ellipsis.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        format!("{}...", text.chars().take(max - 3).collect::<String>())
    }
}

fn short_uri(call: &Call) -> String {
    let path = call.uri.split('?').next().unwrap_or_default();
    clean(path)
}

fn host_note(call: &Call) -> String {
    match &call.origin {
        Some(origin) => format!("to {origin}"),
        None => "to the system".to_owned(),
    }
}

/// The ETag variable and the `If-Match` header value for updates and deletes.
fn etag_declaration(out: &mut Out, call: &Call) -> Option<String> {
    if !call.kind.needs_etag() {
        return None;
    }
    match &call.if_match {
        Some(value) => Some(literal(value)),
        None => {
            out.line(0, "DATA lv_etag TYPE string.");
            out.comment(
                0,
                "<set lv_etag to the ETag: header 'etag' of a read of this entity>",
            );
            Some("lv_etag".to_owned())
        }
    }
}

fn content_type(call: &Call) -> Option<&str> {
    call.body.as_ref().map(|b| b.content_type.as_str())
}

/// The body text as the snippet sends it: JSON without the whitespace between tokens (an
/// ABAP string literal cannot hold a line break), everything else as written.
fn body_text(call: &Call) -> Option<String> {
    let body = call.body.as_ref()?;
    let text = if call.kind != Kind::BatchRead && body.content_type.contains("json") {
        minify_json(&body.text)
    } else {
        body.text.clone()
    };
    Some(text.replace("\r\n", "\n"))
}

/// The variable that joins the lines of a body that has line breaks, if it has any: CRLF for
/// a batch (multipart bodies need it), a line feed otherwise.
fn line_break(call: &Call) -> Option<&'static str> {
    let text = body_text(call)?;
    if call.kind == Kind::BatchRead {
        Some("lv_crlf")
    } else {
        text.trim_end_matches('\n')
            .contains('\n')
            .then_some("lv_nl")
    }
}

fn declare_line_break(out: &mut Out, call: &Call) {
    match line_break(call) {
        Some("lv_crlf") => out.line(0, "DATA(lv_crlf) = cl_abap_char_utilities=>cr_lf."),
        Some(_) => out.line(0, "DATA(lv_nl) = cl_abap_char_utilities=>newline."),
        None => {}
    }
}

/// `lv_body = ...`: one string, or one line after the other joined with the line break.
fn body_assignment(out: &mut Out, indent: usize, call: &Call) {
    let (Some(text), lb) = (body_text(call), line_break(call)) else {
        return;
    };
    match lb {
        None => out.assign_string(indent, "lv_body =", text.trim_end_matches('\n')),
        Some(lb) => {
            out.line(indent, "lv_body = ``.");
            for line in text.trim_end_matches('\n').split('\n') {
                let mut parts = vec!["lv_body".to_owned()];
                parts.extend(chunks(line).iter().map(|c| literal(c)));
                parts.push(lb.to_owned());
                out.assign_parts(indent, "lv_body =", &parts);
            }
        }
    }
}

/// JSON text without whitespace outside of strings (the text is otherwise unchanged).
fn minify_json(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let (mut in_string, mut escaped) = (false, false);
    for c in text.chars() {
        if in_string {
            out.push(c);
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if !c.is_whitespace() {
            out.push(c);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------
// ABAP Platform (classic): cl_http_client through an RFC destination

fn classic(out: &mut Out, call: &Call) {
    out.comment(
        0,
        "Logon data stays in the RFC destination (SM59, connection type G).",
    );
    out.line(0, "");
    out.line(
        0,
        "CONSTANTS lc_destination TYPE rfcdest VALUE '<destination>'.",
    );
    out.comment(0, format!("<RFC destination {}>", host_note(call)));
    out.line(0, "");
    out.line(0, "DATA lo_client TYPE REF TO if_http_client.");
    out.line(0, "DATA lv_uri TYPE string.");
    out.line(0, "DATA lv_status TYPE i.");
    out.line(0, "DATA lv_response TYPE string.");
    out.line(0, "DATA lr_data TYPE REF TO data.");
    if call.kind.needs_token() {
        out.line(0, "DATA lv_csrf TYPE string.");
    }
    if call.body.is_some() {
        out.line(0, "DATA lv_body TYPE string.");
        declare_line_break(out, call);
    }
    let etag = etag_declaration(out, call);
    out.line(0, "");

    out.assign_string(0, "lv_uri =", &call.uri);
    body_assignment(out, 0, call);
    out.line(0, "");

    out.line(0, "cl_http_client=>create_by_destination(");
    out.line(1, "EXPORTING");
    out.line(2, "destination = lc_destination");
    out.line(1, "IMPORTING");
    out.line(2, "client      = lo_client");
    out.line(1, "EXCEPTIONS");
    out.line(2, "OTHERS      = 1 ).");
    out.line(0, "IF sy-subrc <> 0.");
    out.line(1, "RETURN.");
    out.line(0, "ENDIF.");
    out.line(
        0,
        "lo_client->propertytype_accept_cookie = if_http_client=>co_enabled.",
    );
    out.line(0, "");

    if call.kind.needs_token() {
        out.comment(
            0,
            "1. Fetch the CSRF token (GET on the service root). It is read at run time.",
        );
        classic_exchange(
            out,
            call,
            "GET",
            &format!("`{}`", call.service_root),
            &[("x-csrf-token", "`fetch`".into())],
            None,
        );
        out.line(
            0,
            "lv_csrf = lo_client->response->get_header_field( 'x-csrf-token' ).",
        );
        out.line(0, "lo_client->refresh_request( ).");
        out.line(0, "");
        out.comment(0, "2. The change, in the same session.");
    }

    let mut headers: Vec<(&str, String)> = vec![("Accept", "`application/json`".into())];
    for (n, v) in &call.headers {
        if !n.eq_ignore_ascii_case("accept") && !n.eq_ignore_ascii_case("x-http-method") {
            headers.push((n.as_str(), literal(v)));
        }
    }
    if let Some((_, v)) = call
        .headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("accept"))
    {
        headers[0].1 = literal(v);
    }
    if call.kind.needs_token() {
        headers.push(("x-csrf-token", "lv_csrf".into()));
    }
    if let Some(etag) = &etag {
        headers.push(("If-Match", etag.clone()));
    }
    if let Some(ct) = content_type(call) {
        headers.push(("Content-Type", literal(ct)));
    }
    let body = call.body.as_ref().map(|_| "lv_body");
    classic_exchange(out, call, &call.method, "lv_uri", &headers, body);

    out.line(
        0,
        "lo_client->response->get_status( IMPORTING code = lv_status ).",
    );
    out.line(0, "lv_response = lo_client->response->get_cdata( ).");
    out.line(0, "lo_client->close( ).");
    out.line(0, "");
    out.line(
        0,
        "IF lv_status BETWEEN 200 AND 299 AND lv_response IS NOT INITIAL.",
    );
    out.line(1, "/ui2/cl_json=>deserialize(");
    out.line(2, "EXPORTING json = lv_response");
    out.line(2, "CHANGING  data = lr_data ).");
    out.line(0, "ENDIF.");
    if call.kind == Kind::BatchRead {
        out.comment(
            0,
            "A batch answers 202 with a multipart body: split lv_response at its boundary.",
        );
    }
}

/// One request/response round trip of the classic client.
fn classic_exchange(
    out: &mut Out,
    _call: &Call,
    method: &str,
    uri: &str,
    headers: &[(&str, String)],
    body: Option<&str>,
) {
    let method = if method == "HEAD" { "GET" } else { method };
    out.line(0, format!("lo_client->request->set_method( '{method}' )."));
    out.line(0, "cl_http_utility=>set_request_uri(");
    out.line(1, "request = lo_client->request");
    out.line(1, format!("uri     = {uri} )."));
    for (name, value) in headers {
        out.line(0, "lo_client->request->set_header_field(");
        out.line(1, format!("name  = '{name}'"));
        out.line(1, format!("value = {value} )."));
    }
    if let Some(body) = body {
        out.line(0, format!("lo_client->request->set_cdata( {body} )."));
    }
    out.line(0, "lo_client->send( EXCEPTIONS OTHERS = 1 ).");
    out.line(0, "IF sy-subrc = 0.");
    out.line(1, "lo_client->receive( EXCEPTIONS OTHERS = 1 ).");
    out.line(0, "ENDIF.");
    out.line(0, "IF sy-subrc <> 0.");
    out.line(1, "lo_client->close( ).");
    out.line(1, "RETURN.");
    out.line(0, "ENDIF.");
}

// ---------------------------------------------------------------------------------------
// ABAP Cloud: cl_web_http_client_manager through a communication arrangement

fn cloud(out: &mut Out, call: &Call) {
    out.comment(
        0,
        "Logon data stays in the communication arrangement (Fiori app \"Communication",
    );
    out.comment(
        0,
        "Arrangements\"). The outbound service is called through it.",
    );
    out.line(0, "");
    out.line(
        0,
        "CONSTANTS lc_comm_scenario TYPE if_com_management=>ty_cscn_id",
    );
    out.line(1, "VALUE '<communication scenario>'.");
    out.line(
        0,
        "CONSTANTS lc_service_id TYPE if_com_management=>ty_cs_id",
    );
    out.line(1, "VALUE '<outbound service>'.");
    out.comment(
        0,
        format!(
            "<scenario and service of the arrangement {}>",
            host_note(call)
        ),
    );
    out.line(0, "");
    out.line(0, "DATA lv_uri TYPE string.");
    if call.kind.needs_token() {
        out.line(0, "DATA lv_csrf TYPE string.");
    }
    if call.body.is_some() {
        out.line(0, "DATA lv_body TYPE string.");
        declare_line_break(out, call);
    }
    out.line(0, "DATA lr_data TYPE REF TO data.");
    let etag = etag_declaration(out, call);
    out.line(0, "");
    out.assign_string(0, "lv_uri =", &call.uri);
    body_assignment(out, 0, call);
    out.line(0, "");

    out.line(0, "TRY.");
    out.line(
        1,
        "DATA(lo_destination) = cl_http_destination_provider=>create_by_comm_arrangement(",
    );
    out.line(2, "comm_scenario = lc_comm_scenario");
    out.line(2, "service_id    = lc_service_id ).");
    out.line(1, "DATA(lo_client) = cl_web_http_client_manager=>create_by_http_destination( lo_destination ).");
    out.line(1, "DATA(lo_request) = lo_client->get_http_request( ).");
    out.line(1, "");

    if call.kind.needs_token() {
        out.comment(
            1,
            "1. Fetch the CSRF token (GET on the service root). It is read at run time.",
        );
        out.line(
            1,
            format!(
                "lo_request->set_uri_path( i_uri_path = {} ).",
                literal(&call.service_root)
            ),
        );
        out.line(
            1,
            "lo_request->set_header_field( i_name = 'x-csrf-token' i_value = `fetch` ).",
        );
        out.line(
            1,
            "DATA(lo_fetch) = lo_client->execute( if_web_http_client=>get ).",
        );
        out.line(1, "lv_csrf = lo_fetch->get_header_field( 'x-csrf-token' ).");
        out.line(1, "");
        out.comment(1, "2. The change, in the same session.");
    }

    out.line(1, "lo_request->set_uri_path( i_uri_path = lv_uri ).");
    let mut headers: Vec<(String, String)> = vec![("Accept".into(), "`application/json`".into())];
    for (n, v) in &call.headers {
        if n.eq_ignore_ascii_case("accept") {
            headers[0].1 = literal(v);
        } else if !n.eq_ignore_ascii_case("x-http-method") {
            headers.push((n.clone(), literal(v)));
        }
    }
    let mut method = call.method.as_str();
    if !matches!(method, "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE") {
        // MERGE: tunnelled through POST.
        headers.push(("X-HTTP-Method".into(), literal(method)));
        method = "POST";
    }
    if call.kind.needs_token() {
        headers.push(("x-csrf-token".into(), "lv_csrf".into()));
    }
    if let Some(etag) = &etag {
        headers.push(("If-Match".into(), etag.clone()));
    }
    if let Some(ct) = content_type(call) {
        headers.push(("Content-Type".into(), literal(ct)));
    }
    for (name, value) in &headers {
        out.line(1, "lo_request->set_header_field(");
        out.line(2, format!("i_name  = '{name}'"));
        out.line(2, format!("i_value = {value} )."));
    }
    if call.body.is_some() {
        out.line(1, "lo_request->set_text( lv_body ).");
    }
    let constant = match method {
        "HEAD" => "get",
        other => &other.to_ascii_lowercase(),
    };
    out.line(
        1,
        format!("DATA(lo_response) = lo_client->execute( if_web_http_client=>{constant} )."),
    );
    out.line(1, "DATA(lv_status) = lo_response->get_status( )-code.");
    out.line(1, "DATA(lv_response) = lo_response->get_text( ).");
    out.line(1, "lo_client->close( ).");
    out.line(1, "");
    out.line(
        1,
        "IF lv_status BETWEEN 200 AND 299 AND lv_response IS NOT INITIAL.",
    );
    out.line(2, "/ui2/cl_json=>deserialize(");
    out.line(3, "EXPORTING json = lv_response");
    out.line(3, "CHANGING  data = lr_data ).");
    out.line(1, "ENDIF.");
    if call.kind == Kind::BatchRead {
        out.comment(
            1,
            "A batch answers 202 with a multipart body: split lv_response at its boundary.",
        );
    }
    out.line(
        0,
        "CATCH cx_http_dest_provider_error cx_web_http_client_error",
    );
    out.line(2, "cx_web_message_error INTO DATA(lx_error).");
    out.comment(1, "Handle the error: lx_error->get_text( ).");
    out.line(0, "ENDTRY.");
}
