// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! OAuth 2.0 for `rt send` (wave 5, strand C): tokens in the OS keychain (shared with the
//! desktop app for the same collection), the system browser plus a loopback listener for
//! the authorization code flow, the device code on stderr. `--no-prompt` fails instead.
//!
//! The token entries are the desktop app's (`rt_app::auth::SecretTokens`, service
//! "RequestTailor", collection = the canonical root path), so a token fetched in the app is
//! used by `rt send` in a pipeline on the same machine and the other way round. Nothing here
//! prints a token: stdout is the report (masked), stderr gets the URL to open or the code.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use rt_app::auth::loopback::{NativePrompt, open_browser};
use rt_app::auth::{AuthPrompt as Prompt, SecretTokens};
use rt_app::secrets::keychain::KeychainStore;
use rt_core::services::{AuthPrompt, TokenStore};

use crate::SendArgs;

pub struct CliAuth {
    tokens: SecretTokens,
    prompt: Option<NativePrompt>,
}

/// `SecretKey::collection` of a collection root, as the desktop app computes it
/// (`apps/desktop/src-tauri/src/files.rs`): the canonical path, without `\\?\` on Windows.
pub fn collection_id(root: &Path) -> String {
    let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let text = canonical.to_string_lossy();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
}

/// What `rt send` writes to stderr while a flow waits for the user.
pub fn prompt_text(prompt: &Prompt) -> Option<String> {
    match prompt {
        Prompt::Browser { url } => Some(format!(
            "OAuth 2.0: sign in in the browser. If it did not open, open this URL:\n  {url}"
        )),
        Prompt::DeviceCode(d) => {
            let minutes = d.expires_in.div_ceil(60);
            let mut text = format!(
                "OAuth 2.0: open {} and enter the code {} (valid for {minutes} min)",
                d.verification_uri, d.user_code
            );
            if let Some(complete) = &d.verification_uri_complete {
                text.push_str(&format!("\n  or open {complete}"));
            }
            Some(text)
        }
        Prompt::Done => None,
    }
}

impl CliAuth {
    pub fn new(args: &SendArgs, root: &Path) -> Self {
        let tokens = SecretTokens::new(Arc::new(KeychainStore::default()), collection_id(root));
        let prompt = args.prompt.then(|| {
            NativePrompt::new(
                |p| {
                    if let Some(text) = prompt_text(p) {
                        eprintln!("{text}");
                    }
                },
                // Ctrl-C ends the process; nothing else cancels.
                Arc::new(AtomicBool::new(false)),
            )
            // Without a browser (SSH, CI) the URL on stderr is enough.
            .with_opener(|url| {
                let _ = open_browser(url);
                Ok(())
            })
        });
        Self { tokens, prompt }
    }

    pub fn tokens(&self) -> Option<&dyn TokenStore> {
        Some(&self.tokens)
    }

    pub fn prompt(&self) -> Option<&dyn AuthPrompt> {
        self.prompt.as_ref().map(|p| p as &dyn AuthPrompt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_on_stderr() {
        assert!(prompt_text(&Prompt::Done).is_none());
        let text = prompt_text(&Prompt::DeviceCode(rt_core::services::DeviceCodePrompt {
            user_code: "WDJB-MJHT".into(),
            verification_uri: "https://idp/device".into(),
            verification_uri_complete: Some("https://idp/device?user_code=WDJB-MJHT".into()),
            expires_in: 599,
        }))
        .unwrap();
        assert!(text.contains("https://idp/device and enter the code WDJB-MJHT"));
        assert!(text.contains("10 min"));
        assert!(text.contains("?user_code=WDJB-MJHT"));
        let text = prompt_text(&Prompt::Browser {
            url: "https://idp/authorize?client_id=c".into(),
        })
        .unwrap();
        assert!(text.ends_with("https://idp/authorize?client_id=c"));
    }

    #[test]
    fn no_prompt_switches_the_prompt_off_but_keeps_the_cache() {
        let mut args = SendArgs {
            prompt: false,
            ..SendArgs::default()
        };
        let auth = CliAuth::new(&args, Path::new("."));
        assert!(auth.prompt().is_none());
        assert!(auth.tokens().is_some());
        args.prompt = true;
        assert!(CliAuth::new(&args, Path::new(".")).prompt().is_some());
    }

    #[test]
    fn the_collection_id_is_the_canonical_root() {
        let id = collection_id(Path::new("."));
        assert_eq!(
            id,
            std::env::current_dir()
                .unwrap()
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .trim_start_matches(r"\\?\")
        );
    }
}
