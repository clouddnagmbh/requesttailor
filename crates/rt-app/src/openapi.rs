// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The OpenAPI import wizard in the app: fetching a spec from a URL (with the files its
//! relative `$ref`s name) over the core's [`Transport`], reading it for the wizard
//! ([`rt_format::import::inspect_openapi`]) and converting it with the user's choices into an
//! [`ImportPlan`] that is written like any other import (`ImportApi.write` / `writeInto`).
//!
//! The run wizard ("Ready to send?") and the response check are in [`crate::contract`].

use rt_core::transport::{HttpRequest, SendOptions, Transport, collect};
use rt_format::import::{Loss, LossEffect, LossKind, SourceFile};
pub use rt_format::import::{
    OpenApiExamples, OpenApiGrouping, OpenApiNaming, OpenApiOptions, OpenApiServerChoice,
    SpecOperation, SpecOverview, SpecSecurityScheme, SpecServer, SpecServerVariable, SpecTag,
};
use serde::{Deserialize, Serialize};

use crate::{AppError, ImportPlan, Imports};

/// Most files a fetched spec may have (the main file included).
pub const MAX_FETCHED_FILES: usize = 64;
/// Most bytes of all fetched files together.
pub const MAX_FETCHED_BYTES: usize = rt_format::import::MAX_INPUT_BYTES;

/// A spec fetched from a URL: the main file first, then the referenced files, named by their
/// path relative to the main file's URL (as the importer resolves `$ref`s).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchedSpec {
    /// The URL as given (after redirects).
    pub url: String,
    pub files: Vec<FetchedFile>,
    /// References that could not be fetched (or were over the limits), as import losses.
    pub losses: Vec<Loss>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchedFile {
    pub name: String,
    pub text: String,
}

/// Fetches `url` and, recursively, the relative files its `$ref`s name (same origin only,
/// at most [`MAX_FETCHED_FILES`] and [`MAX_FETCHED_BYTES`]). Fails with kind `import` if the
/// main file cannot be fetched (the message names the HTTP status or transport error).
///
/// The main file is named after the last segment of its URL (after redirects), the others by
/// their path relative to the main file's folder (`schemas/pet.yaml`, `../common.yaml`), so
/// the importer resolves the `$ref`s exactly as written. References are followed breadth
/// first in order of appearance; absolute URLs are not followed (the importer reports them),
/// and neither is anything of another origin. A reference that cannot be fetched, is no text
/// or is over the limits becomes a loss (dropped, kind other) naming it.
pub async fn fetch_spec<T: Transport>(transport: &T, url: &str) -> Result<FetchedSpec, AppError> {
    let start = url::Url::parse(url.trim())
        .map_err(|e| AppError::Import(format!("{url} is not a valid URL: {e}")))?;
    if !matches!(start.scheme(), "http" | "https") {
        return Err(AppError::Import(format!(
            "{url}: only http and https URLs can be fetched"
        )));
    }
    let (main_url, text) = get_text(transport, &start, MAX_FETCHED_BYTES)
        .await
        .map_err(|e| AppError::Import(format!("cannot fetch {url}: {e}")))?;
    let main_name = main_url
        .path_segments()
        .and_then(|mut s| s.next_back())
        .map(decode)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "openapi.yaml".to_owned());
    let mut out = FetchedSpec {
        url: main_url.to_string(),
        files: Vec::new(),
        losses: Vec::new(),
    };
    let mut total = text.len();
    let mut queue = std::collections::VecDeque::new();
    queue.extend(relative_refs(&main_url, &main_name, &text));
    out.files.push(FetchedFile {
        name: main_name,
        text,
    });
    let mut seen: Vec<url::Url> = vec![without_fragment(&main_url)];
    while let Some(target) = queue.pop_front() {
        if seen.contains(&target) {
            continue;
        }
        seen.push(target.clone());
        let Some(name) = relative_name(&main_url, &target) else {
            continue;
        };
        if target.origin() != main_url.origin() {
            continue;
        }
        if out.files.len() >= MAX_FETCHED_FILES {
            out.losses.push(dropped(format!(
                "`{name}` not fetched: a spec may have at most {MAX_FETCHED_FILES} files"
            )));
            continue;
        }
        let room = MAX_FETCHED_BYTES.saturating_sub(total);
        match get_text(transport, &target, room).await {
            Ok((_, text)) => {
                total += text.len();
                queue.extend(relative_refs(&target, &name, &text));
                out.files.push(FetchedFile { name, text });
            }
            Err(e) => out
                .losses
                .push(dropped(format!("`{name}` could not be fetched ({e})"))),
        }
    }
    Ok(out)
}

fn dropped(detail: String) -> Loss {
    Loss {
        location: "collection".into(),
        kind: LossKind::Other,
        effect: LossEffect::Dropped,
        detail,
    }
}

fn decode(segment: &str) -> String {
    url::form_urlencoded::parse(format!("x={}", segment.replace('+', "%2B")).as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| segment.to_owned())
}

fn without_fragment(u: &url::Url) -> url::Url {
    let mut u = u.clone();
    u.set_fragment(None);
    u
}

/// The relative references of the file `name` (at `at`), as URLs to fetch.
fn relative_refs(at: &url::Url, name: &str, text: &str) -> Vec<url::Url> {
    let refs = rt_format::import::openapi_references(&SourceFile {
        name,
        data: text.as_bytes(),
    });
    refs.iter()
        .filter(|r| url::Url::parse(r).is_err() && !r.starts_with("//"))
        .filter_map(|r| at.join(r).ok())
        .map(|u| without_fragment(&u))
        .collect()
}

/// `target`'s path relative to the folder of `main` (`schemas/pet.yaml`, `../a.yaml`).
fn relative_name(main: &url::Url, target: &url::Url) -> Option<String> {
    let dir: Vec<String> = {
        let mut segs: Vec<String> = main.path_segments()?.map(decode).collect();
        segs.pop();
        segs
    };
    let path: Vec<String> = target.path_segments()?.map(decode).collect();
    let common = dir
        .iter()
        .zip(path.iter())
        .take_while(|(a, b)| a == b)
        .count()
        .min(path.len().saturating_sub(1));
    let mut parts: Vec<String> = vec!["..".to_owned(); dir.len() - common];
    parts.extend(path[common..].iter().cloned());
    let name = parts.join("/");
    (!name.is_empty() && !path.last().is_some_and(String::is_empty)).then_some(name)
}

/// GETs `url` and returns the final URL and the body as text (at most `limit` bytes).
async fn get_text<T: Transport>(
    transport: &T,
    url: &url::Url,
    limit: usize,
) -> Result<(url::Url, String), String> {
    let request = HttpRequest {
        method: "GET".into(),
        url: url.to_string(),
        headers: vec![(
            "Accept".into(),
            "application/yaml, application/json;q=0.9, */*;q=0.8".into(),
        )],
        body: None,
        options: SendOptions::default(),
    };
    let started = web_time::Instant::now();
    let response = transport.send(request).await.map_err(|e| e.to_string())?;
    let done = collect(response, limit, started)
        .await
        .map_err(|e| e.to_string())?;
    if !(200..300).contains(&done.head.status) {
        return Err(format!("HTTP {}", done.head.status));
    }
    if done.truncated {
        return Err(format!(
            "larger than {} MB together",
            MAX_FETCHED_BYTES / (1024 * 1024)
        ));
    }
    let text = String::from_utf8(done.body.to_vec()).map_err(|_| "not UTF-8 text".to_owned())?;
    let final_url = url::Url::parse(&done.head.url).unwrap_or_else(|_| url.clone());
    Ok((final_url, text))
}

/// [`rt_format::import::inspect_openapi`] with the error as an [`AppError`].
pub fn inspect(files: &[SourceFile<'_>]) -> Result<SpecOverview, AppError> {
    rt_format::import::inspect_openapi(files).map_err(|e| AppError::Import(e.to_string()))
}

impl Imports {
    /// Converts the spec in `files` with the wizard's `options` and keeps the result as a plan
    /// (written with `take` / [`crate::import::add_to_workspace`] like any import).
    pub fn analyze_openapi(
        &mut self,
        files: &[SourceFile<'_>],
        options: &OpenApiOptions,
    ) -> Result<ImportPlan, AppError> {
        let import = rt_format::import::import_openapi(files, options)
            .map_err(|e| AppError::Import(e.to_string()))?;
        Ok(self.add(import))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use bytes::Bytes;
    use rt_core::transport::{HttpVersion, Response, ResponseHead, TransportError};

    use super::*;

    fn block_on<F: std::future::Future>(fut: F) -> F::Output {
        let mut fut = std::pin::pin!(fut);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        loop {
            if let std::task::Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    struct Once(Option<Bytes>);

    impl futures_core::Stream for Once {
        type Item = Result<Bytes, TransportError>;
        fn poll_next(
            mut self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            std::task::Poll::Ready(self.0.take().map(Ok))
        }
    }

    /// Serves fixed answers by URL (no network); unknown URLs are 404. A `→` entry redirects.
    struct Fake {
        pages: HashMap<String, (u16, String)>,
        redirects: HashMap<String, String>,
        seen: RefCell<Vec<String>>,
    }

    impl Fake {
        fn new(pages: &[(&str, u16, &str)]) -> Self {
            Self {
                pages: pages
                    .iter()
                    .map(|(u, s, b)| ((*u).to_owned(), (*s, (*b).to_owned())))
                    .collect(),
                redirects: HashMap::new(),
                seen: RefCell::new(Vec::new()),
            }
        }
    }

    impl Transport for Fake {
        async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
            self.seen.borrow_mut().push(request.url.clone());
            if request.url.contains("unreachable") {
                return Err(TransportError::Connect("connection refused".into()));
            }
            let url = self
                .redirects
                .get(&request.url)
                .cloned()
                .unwrap_or(request.url);
            let (status, body) = self
                .pages
                .get(&url)
                .cloned()
                .unwrap_or((404, "not found".into()));
            Ok(Response {
                head: ResponseHead {
                    status,
                    version: HttpVersion::Http11,
                    headers: vec![],
                    url,
                    redirects: vec![],
                    timing: Default::default(),
                },
                body: Box::pin(Once(Some(Bytes::from(body)))),
            })
        }
    }

    const MAIN: &str = r##"openapi: 3.0.3
info: {title: Kennel, version: "1"}
paths:
  /dogs:
    get:
      parameters:
        - $ref: "parameters.yaml#/limit"
      responses:
        "200":
          description: ok
          content:
            application/json:
              schema: {$ref: "schemas/dog.yaml"}
        "404":
          description: missing
          content:
            application/json:
              schema: {$ref: "schemas/missing.yaml#/X"}
        "500":
          description: elsewhere
          content:
            application/json:
              schema: {$ref: "https://other.example.com/error.yaml"}
"##;

    fn kennel() -> Fake {
        Fake::new(&[
            ("https://api.example.com/v1/openapi.yaml", 200, MAIN),
            (
                "https://api.example.com/v1/parameters.yaml",
                200,
                "limit: {name: limit, in: query, schema: {type: integer}}\n",
            ),
            (
                "https://api.example.com/v1/schemas/dog.yaml",
                200,
                "type: object\nproperties:\n  owner: {$ref: \"../../common/owner.yaml\"}\n  self: {$ref: \"#/properties\"}\n",
            ),
            (
                "https://api.example.com/common/owner.yaml",
                200,
                "type: object\nproperties:\n  dog: {$ref: \"../v1/schemas/dog.yaml\"}\n",
            ),
        ])
    }

    #[test]
    fn fetches_the_main_file_and_its_relative_references() {
        let t = kennel();
        let spec = block_on(fetch_spec(&t, "https://api.example.com/v1/openapi.yaml")).unwrap();
        let names: Vec<&str> = spec.files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "openapi.yaml",
                "parameters.yaml",
                "schemas/dog.yaml",
                "../common/owner.yaml"
            ]
        );
        assert_eq!(spec.url, "https://api.example.com/v1/openapi.yaml");
        assert_eq!(spec.losses.len(), 1);
        assert_eq!(
            spec.losses[0].detail,
            "`schemas/missing.yaml` could not be fetched (HTTP 404)"
        );
        // Every file once, nothing of another origin, the cycle back to dog.yaml ends.
        let seen = t.seen.borrow();
        assert_eq!(seen.len(), 5, "{seen:?}");
        assert!(!seen.iter().any(|u| u.contains("other.example.com")));
        // The fetched files import as a multi-file spec.
        let sources: Vec<SourceFile<'_>> = spec
            .files
            .iter()
            .map(|f| SourceFile {
                name: &f.name,
                data: f.text.as_bytes(),
            })
            .collect();
        let overview = inspect(&sources).unwrap();
        assert_eq!((overview.title.as_str(), overview.files), ("Kennel", 4));
    }

    #[test]
    fn the_main_file_must_be_fetched() {
        let t = kennel();
        let e = block_on(fetch_spec(&t, "https://api.example.com/v1/nope.yaml")).unwrap_err();
        assert!(
            matches!(&e, AppError::Import(m) if m.contains("HTTP 404")),
            "{e:?}"
        );
        let e = block_on(fetch_spec(&t, "https://unreachable.example.com/a.yaml")).unwrap_err();
        assert!(
            matches!(&e, AppError::Import(m) if m.contains("connection refused")),
            "{e:?}"
        );
        let e = block_on(fetch_spec(&t, "file:///etc/passwd")).unwrap_err();
        assert!(matches!(e, AppError::Import(_)));
        assert!(block_on(fetch_spec(&t, "not a url")).is_err());
    }

    #[test]
    fn redirects_name_files_after_the_final_url() {
        let mut t = kennel();
        t.redirects.insert(
            "https://api.example.com/latest".into(),
            "https://api.example.com/v1/openapi.yaml".into(),
        );
        let spec = block_on(fetch_spec(&t, "https://api.example.com/latest")).unwrap();
        assert_eq!(spec.url, "https://api.example.com/v1/openapi.yaml");
        assert_eq!(spec.files[0].name, "openapi.yaml");
        assert_eq!(spec.files[1].name, "parameters.yaml");
    }

    #[test]
    fn the_number_of_files_is_bounded() {
        // A chain of files, each referencing the next.
        let mut pages = vec![(
            "https://x.example.com/openapi.json".to_owned(),
            200,
            r#"{"openapi": "3.0.3", "info": {"title": "T", "version": "1"}, "paths": {}, "x": {"$ref": "f0.json"}}"#.to_owned(),
        )];
        for i in 0..MAX_FETCHED_FILES + 5 {
            pages.push((
                format!("https://x.example.com/f{i}.json"),
                200,
                format!(r#"{{"$ref": "f{}.json"}}"#, i + 1),
            ));
        }
        let pages: Vec<(&str, u16, &str)> = pages
            .iter()
            .map(|(u, s, b)| (u.as_str(), *s, b.as_str()))
            .collect();
        let t = Fake::new(&pages);
        let spec = block_on(fetch_spec(&t, "https://x.example.com/openapi.json")).unwrap();
        assert_eq!(spec.files.len(), MAX_FETCHED_FILES);
        assert!(
            spec.losses[0].detail.contains("at most"),
            "{:?}",
            spec.losses
        );
    }
}
