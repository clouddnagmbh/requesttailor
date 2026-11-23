// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The prompt of each task: one fixed system prompt, one user message with the task, the
//! masked context as JSON and the shape of the JSON answer (`answer` parses it).

use super::{AiCall, AiContext, AiMessage, AiRole, AiTask};

/// Output budget of one answer (non-streaming call).
pub const MAX_TOKENS: u32 = 16_000;

const SYSTEM: &str = "You are the assistant built into RequestTailor, an API client. A \
collection is a folder of HTTP requests stored as files; requests use variables written as \
{{name}}.\n\
\n\
Rules:\n\
- Values shown as *** are secrets or credentials that were masked before this message was \
sent. Never guess or reconstruct them and never put a credential into an answer; refer to \
variables as {{name}} instead.\n\
- Use the collection's variables where they fit (for example {{baseUrl}} for the host) \
rather than inventing hosts.\n\
- Answer with exactly one fenced code block tagged json holding one JSON object of the shape \
the task asks for, and nothing else. Text fields are Markdown, short and concrete.";

fn context_json(context: &AiContext) -> String {
    serde_json::to_string_pretty(context).unwrap_or_else(|_| "{}".into())
}

const CREATE_SHAPE: &str = r#"{
  "text": "one or two sentences about the request (Markdown)",
  "request": {
    "name": "short name",
    "method": "GET | POST | PUT | PATCH | DELETE | HEAD | OPTIONS",
    "url": "{{baseUrl}}/path?query=value",
    "headers": [{ "name": "Content-Type", "value": "application/json" }],
    "params": [{ "name": "query", "value": "value", "kind": "query | path" }],
    "body": { "type": "none | json | text | xml | form", "data": "body text for json, text, xml", "fields": [{ "name": "", "value": "" }] }
  }
}"#;

const EXPLAIN_SHAPE: &str = r#"{
  "text": "Markdown: the most likely cause first, then concrete steps to fix it"
}"#;

const TESTS_SHAPE: &str = r#"{
  "text": "Markdown: what the tests check",
  "tests": "JavaScript for the request's tests script",
  "assertions": [{ "expression": "res.status", "operator": "eq", "value": "200", "description": "status is OK" }]
}"#;

const TESTS_API: &str = "Tests script API: test(name, fn) defines a test; expect(value) is \
chai's expect (expect(x).to.equal(y), .to.be.a(\"array\"), .to.have.property(\"id\"), \
.to.be.above(n), .to.include(x)); rt.response.status, rt.response.json(), \
rt.response.text(), rt.response.headers.get(name), rt.response.time (ms). Assertions run \
without JavaScript: expression is res.status, res.body.<path> (e.g. res.body.items[0].id, \
res.body.items.length), res.headers.<name> or res.responseTime; operator is one of eq, neq, \
gt, gte, lt, lte, in, notIn, contains, notContains, length, matches, notMatches, startsWith, \
endsWith, between, isEmpty, isNotEmpty, isNull, isUndefined, isDefined, isTruthy, isFalsy, \
isJson, isNumber, isString, isBoolean, isArray; value is the expected value as text (empty \
for unary operators).";

/// The call for `task` (not yet masked).
pub fn build(task: &AiTask, context: &AiContext) -> AiCall {
    let content = match task {
        AiTask::CreateRequest {
            description,
            folder,
        } => format!(
            "Task: write one HTTP request from this description.\n\nDescription:\n{description}\n\n\
Target folder: {}\n\nContext (JSON; siblings are the folder's other requests):\n{}\n\n\
Do not set authentication: the request inherits it from its folder or collection. Query \
parameters belong in the URL; list them in params too. Answer with one json block of this \
shape:\n{CREATE_SHAPE}",
            if folder.is_empty() {
                "(collection root)"
            } else {
                folder
            },
            context_json(context),
        ),
        AiTask::ExplainError { .. } => format!(
            "Task: explain why this request failed (or did not do what was expected) and how \
to fix it: in the request, its variables, auth or the server.\n\nContext (JSON: the request \
as written and the report of what was sent and received):\n{}\n\nAnswer with one json block \
of this shape:\n{EXPLAIN_SHAPE}",
            context_json(context),
        ),
        // Wave 16 tasks: `prepare` refuses them until strand B writes their prompts.
        AiTask::RequestFromSnippet { .. }
        | AiTask::ExplainResponse { .. }
        | AiTask::SuggestAssertions { .. }
        | AiTask::SuggestNames { .. }
        | AiTask::DraftDocs { .. } => String::new(),
        AiTask::SuggestTests { .. } => format!(
            "Task: suggest tests for this request and its response: a tests script and \
declarative assertions. Check what matters for this endpoint (status, the shape and key \
fields of the body, important headers, time). Do not repeat assertions or tests the request \
already has.\n\n{TESTS_API}\n\nContext (JSON: the request as written and the report of what \
was sent and received):\n{}\n\nAnswer with one json block of this shape:\n{TESTS_SHAPE}",
            context_json(context),
        ),
    };
    AiCall {
        system: SYSTEM.to_owned(),
        messages: vec![AiMessage::text(AiRole::User, content)],
        max_tokens: MAX_TOKENS,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_task_names_its_answer_shape_and_carries_the_context() {
        let context = AiContext {
            collection: "Shop".into(),
            variables: vec!["baseUrl".into(), "token (secret)".into()],
            ..Default::default()
        };
        let create = build(
            &AiTask::CreateRequest {
                description: "list orders".into(),
                folder: "orders".into(),
            },
            &context,
        );
        assert_eq!(create.messages.len(), 1);
        let m = &create.messages[0].content;
        assert!(m.contains("list orders") && m.contains("Target folder: orders"));
        assert!(m.contains("\"request\"") && m.contains("token (secret)"));
        let tests = build(
            &AiTask::SuggestTests {
                path: "a.yml".into(),
                response: 1,
            },
            &context,
        );
        assert!(tests.messages[0].content.contains("\"assertions\""));
        assert!(tests.system.contains("***"));
        assert_eq!(tests.max_tokens, MAX_TOKENS);
    }
}
