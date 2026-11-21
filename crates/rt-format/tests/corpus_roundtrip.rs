// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance: reading and writing every collection of the Bruno corpus gives byte-identical
//! files (CLAUDE.md, "Format"). One test per collection, so a failure names the collection.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rt_format::{COLLECTION_FILE, FileSet, read_collection};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/collections/bruno")
}

/// Every directory below `dir` (including `dir`) that contains a collection file. Nested
/// collections (a collection inside another collection's folder) are their own roots.
fn collection_roots(dir: &Path, out: &mut Vec<PathBuf>) {
    if dir.join(COLLECTION_FILE).is_file() {
        out.push(dir.to_path_buf());
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collection_roots(&path, out);
        }
    }
}

/// Reads one corpus collection (and any collection nested in it), writes it back and
/// compares byte for byte. Returns the number of files compared.
fn round_trip(name: &str) -> usize {
    let mut roots = Vec::new();
    collection_roots(&corpus().join(name), &mut roots);
    assert!(!roots.is_empty(), "{name}: no {COLLECTION_FILE}");
    let mut count = 0;
    for root in roots {
        let files = FileSet::from_dir(&root).unwrap();
        let doc = read_collection(&files)
            .unwrap_or_else(|e| panic!("{}: read failed: {e}", root.display()));
        let written = doc.write();
        let mut failures = Vec::new();
        for (path, original) in &files.files {
            match written.files.get(path) {
                None => failures.push(format!("{path}: missing in output")),
                Some(bytes) if bytes != original => failures.push(format!(
                    "{path}: differs\n--- original\n{}\n--- written\n{}",
                    String::from_utf8_lossy(original),
                    String::from_utf8_lossy(bytes)
                )),
                Some(_) => {}
            }
        }
        for path in written.files.keys() {
            if !files.files.contains_key(path) {
                failures.push(format!("{path}: not in input"));
            }
        }
        assert!(
            failures.is_empty(),
            "{}: {} file(s) not byte-identical:\n{}",
            root.display(),
            failures.len(),
            failures.join("\n")
        );
        count += files.files.len();
    }
    count
}

macro_rules! corpus_tests {
    ($($test:ident => $dir:literal,)*) => {
        $(
            #[test]
            fn $test() {
                round_trip($dir);
            }
        )*

        const CORPUS_DIRS: &[&str] = &[$($dir),*];
    };
}

corpus_tests! {
    api_spec_create_from_collection => "api-spec-create-from-collection",
    auth_akamai_edgegrid_yml => "auth-akamai-edgegrid-yml",
    auth_auth_mode_folder_basic_inherit => "auth-auth-mode-folder-basic-inherit",
    auth_auth_mode_folder_bearer_inherit => "auth-auth-mode-folder-bearer-inherit",
    auth_auth_mode_parent_bearer_inherit => "auth-auth-mode-parent-bearer-inherit",
    auth_inherited_auth => "auth-inherited-auth",
    auth_ntlm_ntlm => "auth-ntlm-ntlm",
    auth_ntlm_ntlm_tls => "auth-ntlm-ntlm-tls",
    auth_oauth1_yml => "auth-oauth1-yml",
    collection_change_version => "collection-change-version",
    collection_collection_run_collection_run => "collection-collection-run-collection-run",
    collection_collection_run_delay_run => "collection-collection-run-delay-run",
    collection_collection_run_skipped_run => "collection-collection-run-skipped-run",
    collection_description_yml => "collection-description-yml",
    collection_export => "collection-export",
    collection_invalid_file_indicator_yml => "collection-invalid-file-indicator-yml",
    collection_moving_requests_source_collection => "collection-moving-requests-source-collection",
    collection_moving_requests_target_collection => "collection-moving-requests-target-collection",
    collection_moving_requests_yml_collection => "collection-moving-requests-yml-collection",
    collection_open_deeply_nested_services_identity_internal_v2 => "collection-open-deeply-nested-services-identity-internal-v2",
    collection_open_deeply_nested_services_payments_v1 => "collection-open-deeply-nested-services-payments-v1",
    collection_open_deeply_nested_teams_platform_apis_internal_inventory => "collection-open-deeply-nested-teams-platform-apis-internal-inventory",
    collection_open_multiple_collection_one => "collection-open-multiple-collection-one",
    collection_open_multiple_collection_two => "collection-open-multiple-collection-two",
    collection_open_nested_siblings => "collection-open-nested-siblings",
    collection_remove => "collection-remove",
    collection_zip_git_metadata => "collection-zip-git-metadata",
    environments_api_setenvvar_secret_yml => "environments-api-setenvvar-secret-yml",
    environments_color_file_cache => "environments-color-file-cache",
    environments_datatype_preservation_yml => "environments-datatype-preservation-yml",
    environments_description_yml => "environments-description-yml",
    environments_environment_inheritance_workspace_collections_yml => "environments-environment-inheritance-workspace-collections-yml",
    environments_rename_environment_yml => "environments-rename-environment-yml",
    folder_description_yml => "folder-description-yml",
    folder_folder_actions => "folder-folder-actions",
    folder_script => "folder-script",
    folder_settings_authentication_auth_configure => "folder-settings-authentication-auth-configure",
    folder_tags_yml => "folder-tags-yml",
    folder_tests => "folder-tests",
    folder_vars => "folder-vars",
    graphql_query_builder => "graphql-query-builder",
    grpc_run_lifecycle_hooks => "grpc-run-lifecycle-hooks",
    grpc_subpath => "grpc-subpath",
    mounting_method_indicators_yml => "mounting-method-indicators-yml",
    request_binary_file => "request-binary-file",
    request_description_yml => "request-description-yml",
    request_headers_bulk_edit_headers => "request-headers-bulk-edit-headers",
    request_script_sub_tabs_overflow => "request-script-sub-tabs-overflow",
    request_script_yml => "request-script-yml",
    request_sensitive_field_warnings => "request-sensitive-field-warnings",
    request_settings_requests_settings_yml => "request-settings-requests-settings-yml",
    request_tab_panel_error_boundary_invalid_tags_yml => "request-tab-panel-error-boundary-invalid-tags-yml",
    request_vars => "request-vars",
    response_example_binary_preview => "response-example-binary-preview",
    response_example_try_example => "response-example-try-example",
    response_timeline_headers_yml => "response-timeline-headers-yml",
    response_xml_preview => "response-xml-preview",
    runner_cli_ntlm_collection => "runner-cli-ntlm-collection",
    runner_folder_run => "runner-folder-run",
    runner_folder_tags => "runner-folder-tags",
    runner_responsive_toolbar => "runner-responsive-toolbar",
    scripting_bru_api_dirname_per_script_segment_dirname_filename_test => "scripting-bru-api-dirname-per-script-segment-dirname-filename-test",
    scripting_crlf_line_comment_crlf_line_comment => "scripting-crlf-line-comment-crlf-line-comment",
    scripting_nodevm_developer_mode_async_callbacks => "scripting-nodevm-developer-mode-async-callbacks",
    scripting_nodevm_developer_mode_collection_level_script => "scripting-nodevm-developer-mode-collection-level-script",
    scripting_post_response => "scripting-post-response",
    scripting_pre_request => "scripting-pre-request",
    scripting_quickjs_safe_mode_quickjs_safe_mode => "scripting-quickjs-safe-mode-quickjs-safe-mode",
    scripting_req_api_onfail_workspace_onfail_collection => "scripting-req-api-onfail-workspace-onfail-collection",
    sidebar_empty_state_cta_empty_yml => "sidebar-empty-state-cta-empty-yml",
    sidebar_empty_state_cta_yml_with_folder => "sidebar-empty-state-cta-yml-with-folder",
    sidebar_empty_state_cta_yml_with_js => "sidebar-empty-state-cta-yml-with-js",
    sidebar_empty_state_cta_yml_with_request => "sidebar-empty-state-cta-yml-with-request",
    sse => "sse",
    ssl_client_certs_collections_disabled_yml => "ssl-client-certs-collections-disabled-yml",
    ssl_client_certs_collections_pem_yml => "ssl-client-certs-collections-pem-yml",
    ssl_client_certs_collections_pfx_yml => "ssl-client-certs-collections-pfx-yml",
    ssl_client_certs_tests_collection_client_certs_disabled_yml => "ssl-client-certs-tests-collection-client-certs-disabled-yml",
    ssl_client_certs_tests_collection_client_certs_enabled_yml => "ssl-client-certs-tests-collection-client-certs-enabled-yml",
    unresolved_variables_info => "unresolved-variables-info",
    variable_datatypes_reference_types_workspace_collections_reference_types => "variable-datatypes-reference-types-workspace-collections-reference-types",
    variable_datatypes_workspace_collections_yml => "variable-datatypes-workspace-collections-yml",
    variables_tab_no_variables => "variables-tab-no-variables",
    variables_tab_variables_tab => "variables-tab-variables-tab",
    websockets_multi_message_yml => "websockets-multi-message-yml",
    websockets_variable_interpolation_scopes_workspace_collections_yml => "websockets-variable-interpolation-scopes-workspace-collections-yml",
}

/// The list above must name every corpus collection, and together they must cover every
/// YAML file of the corpus (386 at the time of writing).
#[test]
fn corpus_is_fully_covered() {
    let on_disk: BTreeSet<String> = std::fs::read_dir(corpus())
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().into_string().unwrap())
        .collect();
    let listed: BTreeSet<String> = CORPUS_DIRS.iter().map(|s| s.to_string()).collect();
    assert_eq!(on_disk, listed, "corpus_tests! list is out of date");

    fn count_yml(dir: &Path) -> usize {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .map(|p| {
                if p.is_dir() {
                    count_yml(&p)
                } else {
                    usize::from(p.extension().is_some_and(|x| x == "yml"))
                }
            })
            .sum()
    }
    let compared: usize = CORPUS_DIRS.iter().map(|d| round_trip(d)).sum();
    assert_eq!(compared, count_yml(&corpus()));
    assert_eq!(compared, 386);
}
