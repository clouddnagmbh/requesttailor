// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Compiling proto sources that the caller provides (no file system access here: desktop and
//! CLI read from disk, the web from the opened folder).
//!
//! Paths are `/`-separated and relative to the collection root. Like `protoc`, imports are
//! looked up in the import paths; the directory of every input file is an import path too
//! (what Bruno and most proto layouts expect). `google/protobuf/*.proto` (the well-known
//! types) are built in.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;

use miette::Diagnostic;
use protox::file::{File, FileResolver, GoogleFileResolver};

use crate::GrpcError;

/// `a/./b/../c` → `a/c`; leading `..` stay (a proto file may live outside the collection).
pub fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// `dir` joined with `name` and normalized; `dir` may be empty.
pub fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        normalize(name)
    } else {
        normalize(&format!("{dir}/{name}"))
    }
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Loads a proto source by its normalized path; `None` if there is none.
pub type Loader<'a> = dyn Fn(&str) -> Option<String> + 'a;

/// Paths the resolver has seen: `Some(source)` loaded, `None` known to be missing.
type Cache = Rc<RefCell<BTreeMap<String, Option<String>>>>;

/// `protox` wants a `'static` resolver, so sources are loaded between compile attempts: the
/// resolver records paths it has not seen (`wanted`), they are loaded, and the compile runs
/// again until nothing new is wanted.
struct Resolver {
    roots: Vec<String>,
    cache: Cache,
    wanted: Rc<RefCell<Vec<String>>>,
    google: GoogleFileResolver,
    /// Logical name → path, for error messages.
    opened: Rc<RefCell<BTreeMap<String, String>>>,
}

/// The logical (import) name of an input file: relative to the first import path that
/// contains it.
fn logical_name(roots: &[String], path: &str) -> String {
    for root in roots {
        if root.is_empty() {
            return path.to_owned();
        }
        if let Some(rest) = path.strip_prefix(&format!("{root}/")) {
            return rest.to_owned();
        }
    }
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

impl FileResolver for Resolver {
    fn resolve_path(&self, path: &Path) -> Option<String> {
        let path = normalize(&path.to_string_lossy());
        Some(logical_name(&self.roots, &path))
    }

    fn open_file(&self, name: &str) -> Result<File, protox::Error> {
        let mut unknown = false;
        for root in &self.roots {
            let path = join(root, name);
            match self.cache.borrow().get(&path) {
                Some(Some(source)) if !unknown => {
                    self.opened
                        .borrow_mut()
                        .insert(name.to_owned(), path.clone());
                    return File::from_source(name, source);
                }
                Some(_) => {}
                None => {
                    unknown = true;
                    self.wanted.borrow_mut().push(path.clone());
                }
            }
        }
        if !unknown && name.starts_with("google/protobuf/") {
            return self.google.open_file(name);
        }
        Err(protox::Error::file_not_found(name))
    }
}

/// At most this many compile attempts (each loads the imports found missing so far).
const MAX_ROUNDS: usize = 64;

pub(crate) fn compile(
    files: &[String],
    import_paths: &[String],
    load: &Loader<'_>,
) -> Result<prost_reflect::DescriptorPool, GrpcError> {
    if files.is_empty() {
        return Err(GrpcError::Proto("no proto files given".into()));
    }
    let files: Vec<String> = files.iter().map(|f| normalize(f)).collect();
    let mut roots: Vec<String> = import_paths.iter().map(|p| normalize(p)).collect();
    for file in &files {
        let dir = parent(file).to_owned();
        if !roots.contains(&dir) {
            roots.push(dir);
        }
    }
    let cache: Cache = Rc::default();
    for file in &files {
        match load(file) {
            Some(source) => {
                cache.borrow_mut().insert(file.clone(), Some(source));
            }
            None => return Err(GrpcError::Proto(format!("{file}: file not found"))),
        }
    }
    let names: Vec<String> = files.iter().map(|f| logical_name(&roots, f)).collect();
    let mut rounds = 0;
    loop {
        rounds += 1;
        let wanted: Rc<RefCell<Vec<String>>> = Rc::default();
        let opened = Rc::new(RefCell::new(BTreeMap::new()));
        let mut compiler = protox::Compiler::with_file_resolver(Resolver {
            roots: roots.clone(),
            cache: cache.clone(),
            wanted: wanted.clone(),
            google: GoogleFileResolver::new(),
            opened: opened.clone(),
        });
        compiler.include_imports(true);
        let result = files.iter().zip(&names).try_for_each(|(file, name)| {
            compiler.open_file(name).map(|_| ()).map_err(|e| (file, e))
        });
        let wanted = std::mem::take(&mut *wanted.borrow_mut());
        if !wanted.is_empty() && rounds < MAX_ROUNDS {
            for path in wanted {
                let source = load(&path);
                cache.borrow_mut().insert(path, source);
            }
            continue;
        }
        return match result {
            Ok(()) => Ok(compiler.descriptor_pool()),
            Err((file, e)) => Err(proto_error(&e, file, &opened.borrow(), &cache.borrow())),
        };
    }
}

/// `file:line: message`, with the line of the first label when the error has one.
fn proto_error(
    e: &protox::Error,
    input: &str,
    opened: &BTreeMap<String, String>,
    cache: &BTreeMap<String, Option<String>>,
) -> GrpcError {
    let file = e.file().map_or_else(
        || input.to_owned(),
        |name| opened.get(name).cloned().unwrap_or_else(|| name.to_owned()),
    );
    let line = e
        .labels()
        .and_then(|mut labels| labels.next())
        .and_then(|label| {
            let source = cache.get(&file).or_else(|| cache.get(input))?.as_deref()?;
            let offset = label.offset().min(source.len());
            Some(source[..offset].matches('\n').count() + 1)
        });
    let message = e.to_string();
    GrpcError::Proto(match line {
        Some(line) => format!("{file}:{line}: {message}"),
        None if message.contains(&file) => message,
        None => format!("{file}: {message}"),
    })
}

#[cfg(test)]
mod tests {
    use super::{join, normalize};

    #[test]
    fn paths_normalize() {
        assert_eq!(normalize("a/./b/../c.proto"), "a/c.proto");
        assert_eq!(normalize("../x/y.proto"), "../x/y.proto");
        assert_eq!(normalize("a\\b.proto"), "a/b.proto");
        assert_eq!(join("protos", "../common/t.proto"), "common/t.proto");
        assert_eq!(join("", "t.proto"), "t.proto");
    }
}
