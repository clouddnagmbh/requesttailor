// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The prompt of each task: one fixed system prompt, one user message with the task, the
//! masked context as JSON and the shape of the JSON answer (`answer` parses it).

use super::{AiCall, AiContext, AiMessage, AiRole, AiTask, prompts};

/// Output budget of one answer (non-streaming call).
pub const MAX_TOKENS: u32 = 16_000;

fn context_json(context: &AiContext) -> String {
    serde_json::to_string_pretty(context).unwrap_or_else(|_| "{}".into())
}

/// The call for `task` (not yet masked). The texts live in `prompts/*.md`; `prompts::render`
/// inserts what the task brings.
pub fn build(task: &AiTask, context: &AiContext) -> AiCall {
    let context = context_json(context);
    let folder_label = |folder: &str| {
        if folder.is_empty() {
            "(collection root)".to_owned()
        } else {
            folder.to_owned()
        }
    };
    let content = match task {
        AiTask::CreateRequest {
            description,
            folder,
        } => prompts::render(
            task.prompt_id(),
            &[
                ("description", description),
                ("folder", &folder_label(folder)),
                ("context", &context),
            ],
        ),
        AiTask::RequestFromSnippet { snippet, folder } => prompts::render(
            task.prompt_id(),
            &[
                ("snippet", snippet),
                ("folder", &folder_label(folder)),
                ("context", &context),
            ],
        ),
        AiTask::SuggestNames { target, .. } => prompts::render(
            task.prompt_id(),
            &[("ask", target.ask()), ("context", &context)],
        ),
        AiTask::ExplainError { .. }
        | AiTask::SuggestTests { .. }
        | AiTask::ExplainResponse { .. }
        | AiTask::SuggestAssertions { .. }
        | AiTask::DraftDocs { .. } => prompts::render(task.prompt_id(), &[("context", &context)]),
    };
    AiCall {
        system: prompts::render("system", &[]),
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
