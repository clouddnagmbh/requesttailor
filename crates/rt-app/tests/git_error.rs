// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Seam N7: the Git core's errors reach the UI with the same kind and text as before the seam,
//! and an app error raised inside the core (by a signer or credential store) comes back unchanged.

use rt_app::AppError;
use tk_git_types::GitError;

#[test]
fn every_git_error_keeps_its_kind_and_text() {
    let cases = [
        (
            GitError::failed("cannot push: rejected"),
            AppError::Git("cannot push: rejected".into()),
        ),
        (
            GitError::Auth("example.com".into()),
            AppError::GitAuth("example.com".into()),
        ),
        (
            GitError::SecretsFound("1 secret".into()),
            AppError::SecretsFound("1 secret".into()),
        ),
        (
            GitError::Unsupported("ssh".into()),
            AppError::Unsupported("ssh".into()),
        ),
        (
            GitError::Io("cannot read".into()),
            AppError::Shell("cannot read".into()),
        ),
    ];
    for (git, app) in cases {
        let text = git.to_string();
        let converted = AppError::from(git);
        assert_eq!(converted, app);
        assert_eq!(
            converted.to_string(),
            text,
            "the message the user reads is unchanged"
        );
    }
}

#[test]
fn an_app_error_travels_through_the_git_core_unchanged() {
    for app in [
        AppError::Plugin("signer plugin failed".into()),
        AppError::GitAuth("example.com".into()),
        AppError::Shell("keychain locked".into()),
    ] {
        let through: AppError = GitError::from(app.clone()).into();
        assert_eq!(through, app);
    }
}

#[test]
fn a_foreign_host_error_becomes_a_git_error_with_its_text() {
    #[derive(Debug, thiserror::Error)]
    #[error("hardware token missing")]
    struct Token;
    assert_eq!(
        AppError::from(GitError::Host(Box::new(Token))),
        AppError::Git("hardware token missing".into())
    );
}
