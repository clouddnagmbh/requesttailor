// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Variables of the collection and of folders: `request.variables` of `opencollection.yml`
//! and `folder.yml` (variable completion: "Create variable" in a collection or folder).
//!
//! Writing edits only what changed, like the rest of the `write` module: unchanged entries
//! stay byte-identical, edited ones keep their place and the fields the model does not hold.
//! Secret variables are written without a value.

use rt_core::model::Variable;

use crate::runtime::{DEFAULTS_ORDER, RequestPart, apply_request_defaults};
use crate::write::{Empty, strip_secret_values, sync_list, var_list, var_val};
use crate::yaml::{Document, Val};
use crate::{CollectionDoc, FormatError};

/// `request.variables` of a collection or folder file.
fn default_vars(doc: &Document) -> Vec<Variable> {
    crate::read::variables(doc.root.get("request").and_then(|r| r.get("variables")))
}

/// What the file must read back as: secret variables have no value.
fn as_written(vars: &[Variable]) -> Vec<Variable> {
    vars.iter()
        .map(|v| Variable {
            value: if v.secret { None } else { v.value.clone() },
            ..v.clone()
        })
        .collect()
}

pub(crate) fn apply_default_vars(
    doc: &Document,
    collection: bool,
    new: &[Variable],
) -> Result<Document, String> {
    let new = as_written(new);
    let list = var_list("variables", DEFAULTS_ORDER, Empty::Remove);
    apply_request_defaults(
        doc,
        collection,
        &RequestPart {
            what: "variables",
            new_is_empty: new.is_empty(),
            write: &|request, nl| {
                sync_list(request, &list, &new, nl);
                strip_secret_values(request, "variables");
                Ok(())
            },
            fresh: &|| Val::Seq(new.iter().map(var_val).collect()),
            same: &|doc| default_vars(doc) == new,
            describe: &|doc| format!("wanted {new:?}, got {:?}", default_vars(doc)),
        },
    )
}

impl CollectionDoc {
    /// Sets the variables of the collection (`folder` is `""`: `request.variables` of
    /// `opencollection.yml`) or of a folder (`request.variables` of its `folder.yml`, created
    /// if the folder has none). Secret variables are written as `secret: true` without a
    /// value, whatever `value` holds. Everything else in the file stays as it was. The edit
    /// is read back and must give `vars`, otherwise it is refused and the file is left as it
    /// was.
    pub fn set_folder_vars(&mut self, folder: &str, vars: &[Variable]) -> Result<(), FormatError> {
        self.edit_request_defaults(folder, vars.is_empty(), &|doc, collection| {
            apply_default_vars(doc, collection, vars)
        })
    }
}

#[cfg(test)]
mod tests {
    use rt_core::model::{Item, Variable};

    use crate::{FileSet, read_collection};

    fn files(entries: &[(&str, &str)]) -> FileSet {
        let mut set = FileSet::default();
        for (path, text) in entries {
            set.files
                .insert((*path).to_owned(), text.as_bytes().to_vec());
        }
        set
    }

    const COLLECTION: &str = "opencollection: 1.0.0\ninfo:\n  name: Demo\n";

    fn text(doc: &crate::CollectionDoc, path: &str) -> String {
        String::from_utf8(doc.write().files[path].clone()).unwrap()
    }

    #[test]
    fn adds_collection_variables_and_reads_them_back() {
        let mut doc = read_collection(&files(&[("opencollection.yml", COLLECTION)])).unwrap();
        doc.set_folder_vars("", &[Variable::new("apiVersion", "v2")])
            .unwrap();
        assert_eq!(
            doc.model().defaults.vars,
            vec![Variable::new("apiVersion", "v2")]
        );
        let written = text(&doc, "opencollection.yml");
        assert!(written.starts_with(COLLECTION), "{written}");
        assert!(written.contains("request:\n  variables:\n"), "{written}");
        let again = read_collection(&doc.write()).unwrap();
        assert_eq!(again.model().defaults.vars, doc.model().defaults.vars);
    }

    #[test]
    fn keeps_unchanged_entries_byte_identical_and_never_writes_secret_values() {
        let source = "opencollection: 1.0.0\ninfo:\n  name: Demo\nrequest:\n  variables:\n    # keep me\n    - name: a\n      value: \"1\"\n  settings:\n    http:\n      timeout: 5\n";
        let mut doc = read_collection(&files(&[("opencollection.yml", source)])).unwrap();
        let mut vars = doc.model().defaults.vars.clone();
        vars.push(Variable {
            name: "token".into(),
            value: Some("leak".into()),
            enabled: true,
            secret: true,
        });
        doc.set_folder_vars("", &vars).unwrap();
        let written = text(&doc, "opencollection.yml");
        assert!(
            written.contains("    # keep me\n    - name: a\n      value: \"1\"\n"),
            "{written}"
        );
        assert!(written.contains("timeout: 5"), "{written}");
        assert!(!written.contains("leak"), "{written}");
        assert_eq!(doc.model().defaults.vars[1].value, None);
        // Setting what is there changes nothing.
        let before = doc.write();
        let same = doc.model().defaults.vars.clone();
        doc.set_folder_vars("", &same).unwrap();
        assert_eq!(doc.write(), before);
    }

    #[test]
    fn creates_a_folder_file_when_needed() {
        let mut doc = read_collection(&files(&[
            ("opencollection.yml", COLLECTION),
            (
                "users/list.yml",
                "info:\n  name: List\n  type: http\nhttp:\n  method: GET\n  url: \"{{baseUrl}}/users\"\n",
            ),
        ]))
        .unwrap();
        doc.set_folder_vars("users", &[Variable::new("userId", "42")])
            .unwrap();
        let Item::Folder(f) = &doc.model().items[0] else {
            panic!("folder expected");
        };
        assert_eq!(f.defaults.vars, vec![Variable::new("userId", "42")]);
        assert!(text(&doc, "users/folder.yml").contains("name: userId"));
        assert!(doc.set_folder_vars("nope", &[]).is_err());
    }
}
