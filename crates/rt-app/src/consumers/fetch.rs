// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Finding a consumer on this machine (wave 19, strand F; native only): a local path, or a
//! shallow clone of a Git repository into `consumers/<hash>/` of the data folder.
//!
//! The clone runs the `git` program with `--depth 1` for the one ref asked for (`rt-git` has
//! no shallow fetch, no ref choice and no token variable yet). A token for HTTPS comes from
//! `RT_GIT_TOKEN` and goes to `git` through its environment, never an argument and never into
//! a file or report. At most [`MAX_CLONE_BYTES`] of files stay; more and the clone is removed.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rt_format::COLLECTION_FILE;

use super::ConsumerRef;

/// The most one clone may take on disk.
pub const MAX_CLONE_BYTES: u64 = 256 * 1024 * 1024;

/// How long a clone may take.
const CLONE_SECONDS: u64 = 300;

/// Where a consumer is on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    /// The consumer collection's folder.
    pub root: PathBuf,
    /// The commit a clone is at.
    pub commit: Option<String>,
}

/// Whether `source` names a repository on a network rather than a folder on this machine.
pub fn is_remote(source: &str) -> bool {
    let s = source.trim();
    if s.starts_with("file://") {
        return true;
    }
    if s.contains("://") {
        return true;
    }
    // scp-like `git@host:path`, not a Windows drive (`C:\`).
    match (s.find(':'), s.find(['/', '\\'])) {
        (Some(c), slash) => c > 1 && slash.is_none_or(|i| c < i),
        _ => false,
    }
}

fn collection_in(base: &Path, sub: Option<&str>, name: &str) -> Result<PathBuf, String> {
    let mut dir = base.to_path_buf();
    if let Some(sub) = sub {
        if sub.starts_with(['/', '\\']) || sub.split(['/', '\\']).any(|p| p == "..") {
            return Err(format!(
                "consumer {name}: path {sub:?} leaves the repository"
            ));
        }
        dir.push(sub);
    }
    if !dir.join(COLLECTION_FILE).is_file() {
        return Err(format!(
            "consumer {name}: no {COLLECTION_FILE} in {}",
            dir.display()
        ));
    }
    Ok(dir)
}

/// A stable name for the clone of this source, ref and folder.
fn key(consumer: &ConsumerRef) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in consumer
        .source
        .bytes()
        .chain([0])
        .chain(consumer.git_ref.clone().unwrap_or_default().into_bytes())
    {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn dir_size(dir: &Path, limit: u64) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                stack.push(e.path());
            } else {
                total += meta.len();
            }
            if total > limit {
                return total;
            }
        }
    }
    total
}

fn git(dir: &Path, args: &[&str], token: Option<&str>) -> Result<String, String> {
    use base64::Engine as _;
    let mut cmd = Command::new("git");
    cmd.current_dir(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(token) = token {
        let basic = base64::engine::general_purpose::STANDARD.encode(format!("git:{token}"));
        cmd.env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "http.extraHeader")
            .env(
                "GIT_CONFIG_VALUE_0",
                format!("Authorization: Basic {basic}"),
            );
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot run git (needed to clone a consumer): {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(CLONE_SECONDS);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("git {} took more than {CLONE_SECONDS} s", args[0]));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(e.to_string()),
        }
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        let mut message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if let Some(token) = token {
            message = message.replace(token, "***");
        }
        let line = message.lines().last().unwrap_or("").to_owned();
        Err(format!("git {} failed: {line}", args[0]))
    }
}

fn clone(
    consumer: &ConsumerRef,
    target: &Path,
    token: Option<&str>,
    max_bytes: u64,
) -> Result<String, String> {
    let url = consumer.source.trim();
    let reference = consumer.git_ref.as_deref().unwrap_or("HEAD");
    for (what, value) in [("source", url), ("ref", reference)] {
        if value.starts_with('-') || value.contains("::") || value.contains(char::is_control) {
            return Err(format!(
                "consumer {}: the {what} {value:?} is not a repository address or ref",
                consumer.name
            ));
        }
    }
    std::fs::create_dir_all(target).map_err(|e| e.to_string())?;
    git(target, &["init", "-q"], None)?;
    git(target, &["remote", "add", "origin", url], None)?;
    git(
        target,
        &[
            "fetch",
            "-q",
            "--depth",
            "1",
            "--no-tags",
            "origin",
            reference,
        ],
        token,
    )?;
    git(target, &["checkout", "-q", "FETCH_HEAD"], None)?;
    if dir_size(target, max_bytes) > max_bytes {
        return Err(format!(
            "consumer {}: the clone is larger than the limit of {} bytes",
            consumer.name, max_bytes
        ));
    }
    git(target, &["rev-parse", "HEAD"], None)
}

/// Finds the consumer: a local `source` is read relative to `provider_root`; a remote one is
/// cloned shallowly into `<data_dir>/consumers/<hash>/` (kept for the next run unless
/// `refetch`). `token` is the value of `RT_GIT_TOKEN`.
pub fn fetch(
    consumer: &ConsumerRef,
    provider_root: &Path,
    data_dir: Option<&Path>,
    refetch: bool,
    token: Option<&str>,
) -> Result<Fetched, String> {
    fetch_with_limit(
        consumer,
        provider_root,
        data_dir,
        refetch,
        token,
        MAX_CLONE_BYTES,
    )
}

/// [`fetch`] with another size limit for a clone (tests).
pub fn fetch_with_limit(
    consumer: &ConsumerRef,
    provider_root: &Path,
    data_dir: Option<&Path>,
    refetch: bool,
    token: Option<&str>,
    max_clone_bytes: u64,
) -> Result<Fetched, String> {
    if !is_remote(&consumer.source) {
        let base = provider_root.join(consumer.source.trim());
        let base = std::fs::canonicalize(&base).map_err(|e| {
            format!(
                "consumer {}: cannot open {}: {e}",
                consumer.name,
                base.display()
            )
        })?;
        let root = collection_in(&base, consumer.path.as_deref(), &consumer.name)?;
        return Ok(Fetched { root, commit: None });
    }
    let data = data_dir.ok_or_else(|| {
        format!(
            "consumer {}: there is no data folder to clone into (set RT_DATA_DIR)",
            consumer.name
        )
    })?;
    let target = data.join("consumers").join(key(consumer));
    if refetch || !target.join(".git").is_dir() {
        let _ = std::fs::remove_dir_all(&target);
        let commit = clone(consumer, &target, token, max_clone_bytes).inspect_err(|_| {
            let _ = std::fs::remove_dir_all(&target);
        })?;
        let root = collection_in(&target, consumer.path.as_deref(), &consumer.name)?;
        return Ok(Fetched {
            root,
            commit: Some(commit),
        });
    }
    let commit = git(&target, &["rev-parse", "HEAD"], None).ok();
    let root = collection_in(&target, consumer.path.as_deref(), &consumer.name)?;
    Ok(Fetched { root, commit })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_a_network_source_from_a_folder() {
        assert!(is_remote("https://git.example.test/a/b.git"));
        assert!(is_remote("git@host:a/b.git"));
        assert!(is_remote("file:///tmp/x"));
        assert!(!is_remote("../billing"));
        assert!(!is_remote("/srv/consumers/billing"));
        assert!(!is_remote("C:\\work\\billing"));
    }
}
