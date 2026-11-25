// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Contract between the Rust command layer and the TypeScript UI.
//!
//! Serializes one value of every variant the UI receives or sends and writes them, annotated
//! with their TypeScript types, to `apps/ui/src/commands/contract.generated.ts`. `npm run check`
//! (svelte-check) then fails if `types.ts` no longer matches the JSON.
//! This test fails if the committed file is stale; regenerate with
//! `RT_UPDATE_CONTRACT=1 cargo test -p rt-app --test contract`.

use std::time::Duration;

use rt_app::agents::{
    AgentCheck, ApprovalDecision, AuditEntry, AuditOutcome, Effect, PendingApproval, Verdict,
};
use rt_app::ai::{AiAnswer, AiConfig, AiError, AiProviderKind, AiTask};
use rt_app::auth::{AuthPrompt, DeviceCodePrompt, TokenInfo};
use rt_app::cookies::CookieInfo;
use rt_app::git::{
    DiffHunk, DiffLine, DiffLineKind, GitBranch, GitCommit, GitConflict, GitDiff, GitFile,
    GitFileChange, GitFileState, GitIdentity, GitPullResult, GitResolution, GitStatus,
};
use rt_app::mcp_client::{
    McpCallResult, McpContent, McpNotification, McpPrompt, McpPromptArgument, McpPromptMessage,
    McpPromptResult, McpResource, McpResourceContents, McpSelfServer, McpServerConfig,
    McpServerInfo, McpTest, McpTestResult, McpTool,
};
use rt_app::plugins::{
    ActionButton, Block, Contributions, FieldKind, FormField, ImporterContribution, Manifest,
    Permissions, PluginInfo, TabContribution, TabPlace, TabTarget, TreeItem, View, ViewReply,
};
use rt_app::scripts::ScriptTrust;
use rt_app::secret_scan::SecretFinding;
use rt_app::{
    AppError, FileChange, HistoryEntry, ImportPlan, LineSlice, Preview, PreviewVar, SearchField,
    SearchHit, SecretKey, StoredResponse, TreeNode,
};
use rt_core::guard::{Caller, GuardVerdict, Purpose, SendCheck};
use rt_core::model::{
    AgentAccess, AgentPolicy, EnvironmentPolicy, GrpcMethodType, GrpcSpec, MessageKind,
    MessageVariant, Protocol, WebSocketSpec,
};
use rt_core::model::{
    ApiKeyPlacement, Auth, Body, Environment, KeyValue, MultipartPart, Param, ParamKind, PartKind,
    RawKind, RequestSpec, Settings, Variable,
};
use rt_core::model::{Assertion, Script, ScriptPhase, VarAction, VarScope};
use rt_core::model::{
    AwsSigV4, CertificateSource, ClientCertificate, CredentialsPlacement, ExternalSecret,
    ExternalSecrets, OAuth2, OAuth2Flow, OAuth2Param, OAuth2Step, ParamPlacement, PkceMethod,
    SecretProvider, TokenPlacement, TokenSource, TokenUse,
};
use rt_core::runner::{RunEvent, RunPlan, RunStep, RunSummary};
use rt_core::script::{AssertionResult, Flow, LogLevel, ScriptLog, TestResult, VarChange};
use rt_core::send::{
    REPORT_VERSION, ReceivedResponse, ReportBody, SendError, SendReport, SentRequest,
};
use rt_core::services::{Note, NoteLevel};
use rt_core::stream::{Direction, Frame, StreamEvent, StreamHead, StreamLog};
use rt_core::transport::{HttpVersion, Redirect, Timing, TransportError};
use rt_core::vars::{VarSource, VarUse};
use rt_format::import::{ImportFormat, Loss, LossEffect, LossKind, LossReport};
use rt_grpc::{GrpcError, GrpcMethod, GrpcService};
use serde::Serialize;

fn kv(name: &str, value: &str) -> KeyValue {
    KeyValue::new(name, value)
}

fn request_spec() -> RequestSpec {
    RequestSpec {
        protocol: Default::default(),
        scripts: vec![
            Script {
                phase: ScriptPhase::BeforeRequest,
                code: "rt.vars.set('a', '1');".into(),
            },
            Script {
                phase: ScriptPhase::Tests,
                code: "test('ok', () => expect(rt.response.status).to.equal(201));".into(),
            },
        ],
        assertions: vec![Assertion {
            expression: "res.status".into(),
            operator: "eq".into(),
            value: "201".into(),
            description: Some("created".into()),
            enabled: true,
        }],
        actions: vec![VarAction {
            phase: ScriptPhase::AfterResponse,
            expression: "res.body.id".into(),
            method: "jsonq".into(),
            variable: "userId".into(),
            scope: VarScope::Environment,
            enabled: false,
        }],
        name: "Create user".into(),
        seq: Some(2.0),
        method: "POST".into(),
        url: "{{baseUrl}}/users/:id?x=1".into(),
        headers: vec![kv("Accept", "application/json")],
        params: vec![
            Param {
                name: "x".into(),
                value: "1".into(),
                kind: ParamKind::Query,
                enabled: true,
            },
            Param {
                name: "id".into(),
                value: "7".into(),
                kind: ParamKind::Path,
                enabled: false,
            },
        ],
        body: Body::Raw {
            kind: RawKind::Json,
            data: "{}".into(),
        },
        auth: Auth::Bearer {
            token: "{{token}}".into(),
        },
        vars: vec![Variable::new("v", "1")],
        settings: Settings {
            timeout_ms: Some(0),
            follow_redirects: Some(true),
            max_redirects: None,
            encode_url: Some(false),
        },
    }
}

fn bodies() -> Vec<Body> {
    vec![
        Body::None,
        Body::Raw {
            kind: RawKind::Xml,
            data: "<a/>".into(),
        },
        Body::FormUrlEncoded {
            fields: vec![kv("a", "b")],
        },
        Body::Multipart {
            parts: vec![MultipartPart {
                name: "f".into(),
                kind: PartKind::File,
                values: vec!["files/a.txt".into()],
                content_type: Some("text/plain".into()),
                enabled: true,
            }],
        },
        Body::File {
            path: "files/a.bin".into(),
            content_type: None,
        },
        Body::GraphQl {
            query: "query ($id: ID!) { user(id: $id) { name } }".into(),
            variables: "{\"id\": \"{{userId}}\"}".into(),
        },
    ]
}

fn message(title: &str, kind: MessageKind, data: &str) -> MessageVariant {
    MessageVariant {
        title: title.into(),
        selected: true,
        kind,
        data: data.into(),
    }
}

fn protocols() -> Vec<Protocol> {
    vec![
        Protocol::Http,
        Protocol::Grpc(Box::new(GrpcSpec {
            method: "/hello.HelloService/SayHello".into(),
            method_type: GrpcMethodType::BidiStreaming,
            proto_file: Some("protos/hello.proto".into()),
            messages: vec![message(
                "message 1",
                MessageKind::Json,
                "{\"greeting\": \"world\"}",
            )],
        })),
        Protocol::WebSocket(Box::new(WebSocketSpec {
            messages: vec![
                message("", MessageKind::Text, "ping"),
                message("bin", MessageKind::Binary, "AAEC"),
            ],
            timeout_ms: Some(0),
            keep_alive_ms: None,
        })),
    ]
}

fn stream_events() -> Vec<StreamEvent> {
    vec![
        StreamEvent::Opened {
            at_ms: 0,
            head: StreamHead {
                status: 101,
                version: HttpVersion::Http11,
                headers: vec![("upgrade".into(), "websocket".into())],
                url: "wss://echo.example/ws?token=***".into(),
            },
        },
        StreamEvent::Message {
            at_ms: 3,
            direction: Direction::Sent,
            frame: Frame::Text("hello ***".into()),
            name: None,
            id: None,
        },
        StreamEvent::Message {
            at_ms: 9,
            direction: Direction::Received,
            frame: Frame::Binary(vec![0, 1, 2]),
            name: Some("update".into()),
            id: Some("42".into()),
        },
        StreamEvent::Closed {
            at_ms: 12,
            code: Some(1000),
            reason: "bye".into(),
            trailers: vec![("grpc-status".into(), "0".into())],
            by_client: true,
        },
        StreamEvent::Error {
            at_ms: 13,
            error: TransportError::Connect("refused".into()),
        },
    ]
}

fn mcp_result() -> McpCallResult {
    McpCallResult {
        content: vec![
            McpContent::Text {
                text: "{\"ok\":true}".into(),
            },
            McpContent::Image {
                mime_type: "image/png".into(),
                data: "iVBORw0K".into(),
            },
            McpContent::Audio {
                mime_type: "audio/wav".into(),
                data: "UklGRg==".into(),
            },
            McpContent::Resource {
                uri: "file:///a.txt".into(),
                text: None,
            },
            McpContent::Other {
                value: serde_json::json!({"type": "resource_link"}),
            },
        ],
        structured: Some(serde_json::json!({"ok": true})),
        is_error: false,
        time_ms: 12,
        notifications: Vec::new(),
    }
}

fn mcp_test() -> McpTest {
    McpTest {
        name: "lists orders".into(),
        server: McpServerConfig::Stdio {
            command: "node".into(),
            args: vec!["server.js".into()],
            env: vec![("API_TOKEN".into(), "{{token}}".into())],
            cwd: None,
        },
        tool: "list_orders".into(),
        arguments: serde_json::json!({"limit": 5}),
        assertions: vec![Assertion {
            expression: "res.body.ok".into(),
            operator: "eq".into(),
            value: "true".into(),
            description: None,
            enabled: true,
        }],
    }
}

fn auths() -> Vec<Auth> {
    vec![
        Auth::Inherit,
        Auth::None,
        Auth::Basic {
            username: "u".into(),
            password: "{{pw}}".into(),
        },
        Auth::Bearer { token: "t".into() },
        Auth::ApiKey {
            key: "k".into(),
            value: "v".into(),
            placement: ApiKeyPlacement::Query,
        },
        Auth::Unsupported {
            kind: "digest".into(),
        },
        Auth::AwsSigV4(AwsSigV4 {
            access_key_id: "{{awsKey}}".into(),
            secret_access_key: "{{awsSecret}}".into(),
            session_token: String::new(),
            service: "execute-api".into(),
            region: "eu-central-1".into(),
            profile_name: String::new(),
        }),
    ]
    .into_iter()
    .chain(oauth2_flows().into_iter().map(|flow| {
        Auth::OAuth2(Box::new(OAuth2 {
            flow,
            access_token_url: "https://idp/token".into(),
            client_id: "app".into(),
            client_secret: "{{clientSecret}}".into(),
            scope: "openid api".into(),
            parameters: vec![OAuth2Param {
                step: OAuth2Step::AccessToken,
                name: "audience".into(),
                value: "api".into(),
                placement: ParamPlacement::Body,
            }],
            ..OAuth2::default()
        }))
    }))
    .chain([Auth::OAuth2(Box::new(OAuth2 {
        credentials_placement: CredentialsPlacement::Body,
        token: TokenUse {
            id: "shared".into(),
            placement: TokenPlacement::Query {
                name: "access_token".into(),
            },
            source: TokenSource::IdToken,
        },
        auto_fetch: false,
        auto_refresh: false,
        ..OAuth2::default()
    }))])
    .collect()
}

fn oauth2_flows() -> Vec<OAuth2Flow> {
    vec![
        OAuth2Flow::ClientCredentials,
        OAuth2Flow::Password {
            username: "ada".into(),
            password: "{{pw}}".into(),
        },
        OAuth2Flow::AuthorizationCode {
            authorization_url: "https://idp/auth".into(),
            callback_url: "http://127.0.0.1:8765/callback".into(),
            state: String::new(),
            pkce: Some(PkceMethod::S256),
        },
        OAuth2Flow::AuthorizationCode {
            authorization_url: "https://idp/auth".into(),
            callback_url: "https://app/cb".into(),
            state: "s".into(),
            pkce: Some(PkceMethod::Plain),
        },
        OAuth2Flow::AuthorizationCode {
            authorization_url: "https://idp/auth".into(),
            callback_url: "https://app/cb".into(),
            state: String::new(),
            pkce: None,
        },
        OAuth2Flow::Implicit {
            authorization_url: "https://idp/auth".into(),
            callback_url: "https://app/cb".into(),
            state: String::new(),
        },
        OAuth2Flow::DeviceCode {
            device_authorization_url: "https://idp/device".into(),
        },
        OAuth2Flow::Saml2Bearer {
            assertion: "{{samlAssertion}}".into(),
        },
    ]
}

fn environment_with_config() -> Environment {
    Environment {
        client_certificates: vec![
            ClientCertificate {
                domain: "*.example.com".into(),
                source: CertificateSource::Pem {
                    certificate_path: "certs/client.pem".into(),
                    private_key_path: "certs/client.key".into(),
                },
                passphrase: String::new(),
                enabled: true,
            },
            ClientCertificate {
                domain: "api.example.com:8443".into(),
                source: CertificateSource::Pkcs12 {
                    path: "certs/client.p12".into(),
                },
                passphrase: "{{p12Passphrase}}".into(),
                enabled: false,
            },
        ],
        external_secrets: Some(ExternalSecrets {
            provider: SecretProvider::OnePassword,
            variables: vec![ExternalSecret {
                name: "token".into(),
                reference: "op://Dev/API/token".into(),
                enabled: true,
            }],
        }),
        ..environment()
    }
}

fn secret_providers() -> Vec<SecretProvider> {
    vec![
        SecretProvider::HashicorpVaultCloud,
        SecretProvider::HashicorpVaultServer,
        SecretProvider::AwsSecretsManager,
        SecretProvider::AzureKeyVault,
        SecretProvider::GcpSecretsManager,
        SecretProvider::OnePassword,
    ]
}

fn manifest() -> Manifest {
    Manifest {
        id: "jwt-inspector".into(),
        name: "JWT inspector".into(),
        version: "1.0.0".into(),
        api: 1,
        description: "Shows JWT claims.".into(),
        license: Some("Apache-2.0".into()),
        homepage: None,
        permissions: Permissions {
            network: vec!["{request}".into(), "*.example.com".into()],
            storage: true,
            variables: false,
        },
        contributes: Contributions {
            welcome: Some(rt_app::plugins::WelcomeChoice {
                label: "OData".into(),
                description: String::new(),
            }),
            prepare: true,
            response: false,
            tabs: vec![
                TabContribution {
                    id: "claims".into(),
                    title: "JWT".into(),
                    place: TabPlace::Response,
                    content_types: vec!["application/json".into()],
                },
                TabContribution {
                    id: "builder".into(),
                    title: "Query".into(),
                    place: TabPlace::Request,
                    content_types: vec![],
                },
                TabContribution {
                    id: "entities".into(),
                    title: "Entities".into(),
                    place: TabPlace::Collection,
                    content_types: vec![],
                },
            ],
            importers: vec![ImporterContribution {
                id: "edmx".into(),
                label: "OData metadata".into(),
                extensions: vec!["edmx".into(), "xml".into()],
            }],
            paging: Vec::new(),
            compensations: Vec::new(),
        },
    }
}

fn view_reply() -> ViewReply {
    ViewReply {
        create_requests: vec![rt_app::plugins::NewRequest {
            folder: "Books".into(),
            name: "Read Books".into(),
            spec: RequestSpec::default(),
        }],
        add_environments: vec![rt_app::plugins::PluginEnvironment {
            name: "DEV".into(),
            variables: vec![
                rt_app::plugins::PluginVariable {
                    name: "client".into(),
                    value: "100".into(),
                    secret: false,
                },
                rt_app::plugins::PluginVariable {
                    name: "password".into(),
                    value: String::new(),
                    secret: true,
                },
            ],
        }],
        view: View {
            blocks: vec![
                Block::Heading {
                    text: "Claims".into(),
                },
                Block::Text { text: "x".into() },
                Block::Notice {
                    level: NoteLevel::Warning,
                    text: "expired".into(),
                },
                Block::KeyValue {
                    rows: vec![("sub".into(), "ada".into())],
                },
                Block::Table {
                    columns: vec!["a".into(), "b".into()],
                    rows: vec![vec!["1".into(), "2".into()]],
                },
                Block::Code {
                    language: Some("json".into()),
                    text: "{}".into(),
                },
                Block::Code {
                    language: None,
                    text: "x".into(),
                },
                Block::Tree {
                    nodes: vec![TreeItem {
                        label: "Users".into(),
                        detail: Some("EntitySet".into()),
                        children: vec![TreeItem {
                            label: "Id".into(),
                            detail: None,
                            children: vec![],
                            action: Some("select:Id".into()),
                        }],
                        action: None,
                    }],
                },
                Block::Form {
                    id: "q".into(),
                    fields: [FieldKind::Text, FieldKind::Select, FieldKind::Checkbox]
                        .into_iter()
                        .map(|kind| FormField {
                            name: "f".into(),
                            label: "F".into(),
                            kind,
                            value: "".into(),
                            options: vec!["a".into()],
                        })
                        .collect(),
                    submit: "Apply".into(),
                },
                Block::Actions {
                    actions: vec![ActionButton {
                        id: "refresh".into(),
                        label: "Refresh".into(),
                    }],
                },
            ],
        },
        state: Some(serde_json::json!({"page": 2})),
        update_request: Some(request_spec()),
    }
}

fn environment() -> Environment {
    Environment {
        client_certificates: Vec::new(),
        external_secrets: None,
        name: "Local".into(),
        variables: vec![
            Variable::new("baseUrl", "http://127.0.0.1"),
            Variable {
                name: "token".into(),
                value: None,
                enabled: true,
                secret: true,
            },
        ],
    }
}

fn var_sources() -> Vec<VarSource> {
    vec![
        VarSource::Global,
        VarSource::Collection,
        VarSource::Environment {
            name: "Local".into(),
        },
        VarSource::Folder {
            path: "users".into(),
        },
        VarSource::Request,
        VarSource::Runtime,
    ]
}

fn transport_errors() -> Vec<TransportError> {
    vec![
        TransportError::InvalidUrl("x".into()),
        TransportError::Dns("x".into()),
        TransportError::Connect("x".into()),
        TransportError::Tls("x".into()),
        TransportError::Timeout,
        TransportError::TooManyRedirects(5),
        TransportError::Blocked("Failed to fetch".into()),
        TransportError::Other("x".into()),
    ]
}

fn send_errors() -> Vec<SendError> {
    vec![
        SendError::NotFound("a.yml".into()),
        SendError::Prepare("auth type \"oauth2\" is not supported yet".into()),
        SendError::Auth("token endpoint answered 401".into()),
        SendError::Plugin("odata: no CSRF token".into()),
        SendError::Script("ReferenceError: x is not defined".into()),
        SendError::Blocked {
            rule: "hosts.blocked".into(),
            reason: "the host api.example is blocked".into(),
        },
        SendError::Transport(TransportError::Timeout),
    ]
}

fn report_ok() -> SendReport {
    SendReport {
        tests: vec![
            TestResult {
                name: "ok".into(),
                passed: true,
                error: None,
                origin: "request".into(),
            },
            TestResult {
                name: "has id".into(),
                passed: false,
                error: Some("expected undefined to exist".into()),
                origin: "folder:users".into(),
            },
        ],
        assertions: vec![AssertionResult {
            expression: "res.status".into(),
            operator: "eq".into(),
            expected: "201".into(),
            actual: Some("201".into()),
            passed: true,
            error: None,
        }],
        logs: vec![ScriptLog {
            level: LogLevel::Warn,
            text: "token ***".into(),
            origin: "collection".into(),
        }],
        var_changes: vec![
            VarChange {
                name: "a".into(),
                value: Some("1".into()),
                scope: VarScope::Runtime,
            },
            VarChange {
                name: "b".into(),
                value: None,
                scope: VarScope::Global,
            },
        ],
        flow: Some(Flow::Next {
            request: Some("users/list.yml".into()),
        }),
        notes: vec![
            Note {
                source: "auth".into(),
                level: NoteLevel::Info,
                text: "token refreshed".into(),
            },
            Note {
                source: "plugin:odata".into(),
                level: NoteLevel::Error,
                text: "x".into(),
            },
        ],
        version: REPORT_VERSION,
        request_path: "users/create.yml".into(),
        environment: Some("Local".into()),
        request: Some(SentRequest {
            method: "POST".into(),
            url: "http://127.0.0.1/users".into(),
            headers: vec![("Authorization".into(), "Bearer ***".into())],
            body_size: 2,
        }),
        response: Some(ReceivedResponse {
            tls: None,
            status: 201,
            version: HttpVersion::Http2,
            headers: vec![("content-type".into(), "application/json".into())],
            url: "http://127.0.0.1/users".into(),
            redirects: vec![Redirect {
                status: 302,
                location: "http://127.0.0.1/u".into(),
            }],
            body: ReportBody::Utf8("{}".into()),
            size: 2,
            truncated: false,
            timing: Timing {
                dns: Some(Duration::from_micros(1500)),
                connect: Some(Duration::from_millis(2)),
                tls: None,
                ttfb: Duration::from_micros(12_500),
            },
            total: Duration::from_millis(14),
        }),
        variables: var_sources()
            .into_iter()
            .map(|source| VarUse {
                name: "v".into(),
                source,
                secret: false,
            })
            .collect(),
        unresolved: vec!["missing".into()],
        error: None,
        checks: Vec::new(),
        rate_limit: None,
        trace: None,
        segments: Vec::new(),
    }
}

fn run_plan() -> RunPlan {
    RunPlan {
        requests: vec!["users/create.yml".into(), "users/list.yml".into()],
        data: vec![vec![
            ("name".into(), serde_json::json!("Ada")),
            ("age".into(), serde_json::json!(36)),
        ]],
        iterations: 1,
        delay_ms: 10,
        bail: true,
        throttle: Default::default(),
    }
}

fn run_events() -> Vec<RunEvent> {
    let summary = RunSummary {
        requests: 2,
        passed: 1,
        failed: 1,
        skipped: 0,
        tests_passed: 1,
        tests_failed: 1,
        assertions_passed: 1,
        assertions_failed: 0,
        duration_ms: 31,
        stopped: false,
    };
    vec![
        RunEvent::Started { total: 2 },
        RunEvent::Request {
            iteration: 0,
            request_path: "users/create.yml".into(),
        },
        RunEvent::Step {
            step: Box::new(RunStep {
                iteration: 0,
                request_path: "users/create.yml".into(),
                request_name: "Create user".into(),
                report: report_ok(),
                passed: false,
                skipped: false,
            }),
        },
        RunEvent::Finished { summary },
    ]
}

fn report_error() -> SendReport {
    SendReport {
        response: None,
        request: None,
        environment: None,
        error: Some(SendError::Transport(TransportError::Blocked(
            "Failed to fetch".into(),
        ))),
        ..report_ok()
    }
}

fn binary_body() -> ReportBody {
    ReportBody::Base64("AAE=".into())
}

fn tree() -> TreeNode {
    TreeNode::Folder {
        name: "Demo".into(),
        path: "".into(),
        children: vec![
            TreeNode::Folder {
                name: "users".into(),
                path: "users".into(),
                children: vec![],
            },
            TreeNode::Request {
                name: "Health".into(),
                path: "health.yml".into(),
                method: "GET".into(),
            },
        ],
    }
}

fn app_errors() -> Vec<AppError> {
    vec![
        AppError::Format("x".into()),
        AppError::RequestNotFound("a.yml".into()),
        AppError::FolderNotFound("f".into()),
        AppError::ResponseNotFound(3),
        AppError::EnvironmentNotFound("PRD".into()),
        AppError::Filter("x".into()),
        AppError::Import("x".into()),
        AppError::Git("x".into()),
        AppError::GitAuth("github.com".into()),
        AppError::Plugin("x".into()),
        AppError::Auth("x".into()),
        AppError::SecretsFound("x".into()),
        AppError::Review("x".into()),
        AppError::Unsupported("x".into()),
        AppError::Shell("x".into()),
    ]
}

fn git_status() -> GitStatus {
    GitStatus {
        repo: true,
        branch: Some("main".into()),
        upstream: Some("origin/main".into()),
        ahead: 1,
        behind: 0,
        files: [
            GitFileState::Untracked,
            GitFileState::Modified,
            GitFileState::Deleted,
            GitFileState::Conflicted,
        ]
        .into_iter()
        .map(|state| GitFile {
            path: "a.yml".into(),
            state,
        })
        .collect(),
        merging: false,
    }
}

fn git_diff() -> GitDiff {
    GitDiff {
        path: "a.yml".into(),
        binary: false,
        hunks: vec![DiffHunk {
            old_start: 1,
            old_lines: 2,
            new_start: 1,
            new_lines: 2,
            lines: [
                DiffLineKind::Context,
                DiffLineKind::Removed,
                DiffLineKind::Added,
            ]
            .into_iter()
            .map(|kind| DiffLine {
                kind,
                text: "x".into(),
            })
            .collect(),
        }],
    }
}

/// Wave 14: a root with two collections in subfolders (one in Git) and a root that is a
/// collection itself.
fn wave14_commit(oid: &str, summary: &str) -> GitCommit {
    GitCommit {
        oid: oid.repeat(40 / oid.len()),
        summary: summary.into(),
        author: "Ada".into(),
        time: 1_795_000_000,
    }
}

fn git_graph_query() -> rt_app::git::graph::GitGraphQuery {
    rt_app::git::graph::GitGraphQuery {
        skip: 2000,
        limit: 2000,
        refs: Some(vec!["refs/heads/main".into()]),
    }
}

fn git_graph_page() -> rt_app::git::graph::GitGraphPage {
    use rt_app::git::graph::*;
    GitGraphPage {
        commits: vec![
            GitGraphCommit {
                commit: wave14_commit("c", "Merge feature into main"),
                parents: vec!["a".repeat(40), "b".repeat(40)],
                email: "ada@example.com".into(),
                refs: vec!["refs/heads/main".into(), "refs/tags/v1".into()],
                signed: true,
            },
            GitGraphCommit {
                commit: wave14_commit("a", "Initial collection"),
                parents: vec![],
                email: "ada@example.com".into(),
                refs: vec![],
                signed: false,
            },
        ],
        refs: vec![
            GitRef {
                full: "refs/heads/main".into(),
                name: "main".into(),
                kind: GitRefKind::Local,
                target: "c".repeat(40),
                current: true,
                upstream: Some("origin/main".into()),
                ahead: 1,
                behind: 2,
                message: None,
            },
            GitRef {
                full: "refs/remotes/origin/main".into(),
                name: "origin/main".into(),
                kind: GitRefKind::Remote,
                target: "a".repeat(40),
                current: false,
                upstream: None,
                ahead: 0,
                behind: 0,
                message: None,
            },
            GitRef {
                full: "refs/tags/v1".into(),
                name: "v1".into(),
                kind: GitRefKind::Tag,
                target: "c".repeat(40),
                current: false,
                upstream: None,
                ahead: 0,
                behind: 0,
                message: Some("First release".into()),
            },
            GitRef {
                full: "HEAD".into(),
                name: "HEAD".into(),
                kind: GitRefKind::Head,
                target: "c".repeat(40),
                current: false,
                upstream: None,
                ahead: 0,
                behind: 0,
                message: None,
            },
        ],
        head: Some("c".repeat(40)),
        detached: false,
        more: true,
    }
}

fn git_changed_files() -> Vec<rt_app::git::graph::GitChangedFile> {
    use rt_app::git::graph::GitChangedFile;
    vec![
        GitChangedFile {
            path: "/api/orders/create.yml".into(),
            change: GitFileChange::Modified,
            from: None,
            binary: false,
            collection: Some("/work/api".into()),
        },
        GitChangedFile {
            path: "/docs/logo.png".into(),
            change: GitFileChange::Renamed,
            from: Some("/logo.png".into()),
            binary: true,
            collection: None,
        },
    ]
}

fn git_actions() -> Vec<rt_app::git::actions::GitAction> {
    use rt_app::git::actions::{GitAction::*, GitResetMode};
    vec![
        CreateBranch {
            name: "feature/login".into(),
            start: "HEAD".into(),
            switch: true,
        },
        Switch {
            target: "origin/release".into(),
        },
        RenameBranch {
            from: "a".into(),
            to: "b".into(),
        },
        DeleteBranch {
            name: "old".into(),
            force: false,
        },
        Merge {
            from: "feature/login".into(),
        },
        Pull,
        Push,
        Publish {
            branch: "feature/login".into(),
            remote: "origin".into(),
        },
        Fetch { remote: None },
        AddRemote {
            name: "upstream".into(),
            url: "https://git.example.com/a.git".into(),
        },
        RemoveRemote {
            name: "upstream".into(),
        },
        RenameRemote {
            from: "upstream".into(),
            to: "fork".into(),
        },
        SetRemoteUrl {
            name: "origin".into(),
            url: "https://git.example.com/b.git".into(),
        },
        Stash {
            message: Some("half done".into()),
            paths: Vec::new(),
        },
        Stash {
            message: None,
            paths: vec!["/README.md".into()],
        },
        Unstash {
            index: 0,
            keep: false,
        },
        DropStash { index: 1 },
        Revert {
            commit: "c".repeat(40),
        },
        CherryPick {
            commit: "b".repeat(40),
        },
        Reset {
            target: "HEAD~1".into(),
            mode: GitResetMode::KeepChanges,
        },
        Discard {
            paths: vec!["/api/orders/create.yml".into()],
        },
        CreateTag {
            name: "v1".into(),
            target: "HEAD".into(),
            message: Some("First release".into()),
        },
        DeleteTag { name: "v0".into() },
        PushTag {
            name: "v1".into(),
            remote: "origin".into(),
        },
        // Wave 15.
        Amend {
            message: Some("Fix the order total".into()),
            paths: vec!["/src/total.rs".into()],
            staged: false,
        },
        Amend {
            message: None,
            paths: Vec::new(),
            staged: true,
        },
        Abort,
        RestoreBranch {
            name: "feature/login".into(),
            target: "d".repeat(40),
        },
        RestoreFiles {
            rev: "HEAD~2".into(),
            paths: vec!["/docs/guide.md".into()],
        },
        Rewrite {
            branch: "feature/login".into(),
            tip: "e".repeat(40),
            summary: "Squashed 3 commits into one".into(),
        },
    ]
}

fn git_plan() -> rt_app::git::actions::GitPlan {
    use rt_app::git::actions::*;
    GitPlan {
        action: GitAction::Merge {
            from: "feature/login".into(),
        },
        steps: vec!["Brings 2 commits from feature/login into main.".into()],
        refs: vec![GitRefMove {
            name: "refs/heads/main".into(),
            before: Some("a".repeat(40)),
            after: None,
        }],
        files: vec!["/api/orders/create.yml".into()],
        network: false,
        backup: true,
        undoable: true,
        merge: Some(GitMergePreview {
            fast_forward: false,
            commits: vec![wave14_commit("b", "Login")],
            files: vec![
                GitMergeFile {
                    path: "/api/orders/create.yml".into(),
                    semantic: true,
                    clean: 3,
                    conflicts: vec!["http.url".into()],
                },
                GitMergeFile {
                    path: "/README.md".into(),
                    semantic: false,
                    clean: 1,
                    conflicts: vec!["lines 4-9".into()],
                },
            ],
        }),
        destructive: false,
        warnings: vec!["1 field needs your decision.".into()],
        blocked: Some(GitBlock {
            reason: GitBlockReason::UnsavedChanges,
            message: "Save or discard your changes first.".into(),
        }),
    }
}

fn git_action_result() -> rt_app::git::actions::GitActionResult {
    use rt_app::git::actions::*;
    GitActionResult {
        message: "Merged feature/login into main.".into(),
        undo: Some(GitUndoEntry {
            id: "1795000000-1".into(),
            action: "merge".into(),
            summary: "Merge feature/login into main".into(),
            time: 1_795_000_000,
            refs: vec![GitRefMove {
                name: "refs/heads/main".into(),
                before: Some("a".repeat(40)),
                after: Some("c".repeat(40)),
            }],
            backups: vec![backup_ref(1_795_000_000, "main")],
            stash: None,
            stale: Some("main moved since".into()),
        }),
        outcome: Some(GitPullResult::Conflicts {
            paths: vec!["api/orders/create.yml".into()],
        }),
        reload: true,
    }
}

fn git_remotes() -> Vec<rt_app::git::actions::GitRemote> {
    vec![rt_app::git::actions::GitRemote {
        name: "origin".into(),
        url: "https://git.example.com/acme/api.git".into(),
        push_url: Some("git@git.example.com:acme/api.git".into()),
    }]
}

fn git_stashes() -> Vec<rt_app::git::actions::GitStash> {
    vec![rt_app::git::actions::GitStash {
        index: 0,
        oid: "d".repeat(40),
        message: "half done".into(),
        time: 1_795_000_000,
        branch: Some("main".into()),
        files: vec!["/api/orders/create.yml".into()],
    }]
}

fn git_block_reasons() -> Vec<rt_app::git::actions::GitBlockReason> {
    use rt_app::git::actions::GitBlockReason::*;
    vec![
        UnsavedChanges,
        LocalChanges,
        InProgress,
        Detached,
        NoUpstream,
        Diverged,
        NothingToDo,
        InvalidName,
        Exists,
        NotFound,
        CurrentBranch,
        NotMerged,
        Rejected,
        AuthRequired,
        Network,
        Unsupported,
        Published,
    ]
}

fn response_sides() -> rt_app::compare::ResponseSides {
    use rt_app::compare::{ResponseSides, ResponseSnapshot};
    ResponseSides {
        old: Some(ResponseSnapshot {
            status: 200,
            duration_ms: 42,
            size: 18,
            headers: vec![("content-type".into(), "application/json".into())],
            content_type: Some("application/json".into()),
            body: Some("{\"id\":1,\"name\":\"A\"}".into()),
            truncated: false,
        }),
        new: None,
    }
}

fn collection_set() -> rt_app::collections::CollectionSet {
    use rt_app::collections::{CollectionEntry, CollectionRoot, CollectionSet};
    let entry =
        |id: &str, name: &str, root: &str, dir: &str, repo: Option<&str>, dirty| CollectionEntry {
            id: id.into(),
            name: name.into(),
            location: id.into(),
            root: root.into(),
            dir: dir.into(),
            repo: repo.map(Into::into),
            dirty,
        };
    CollectionSet {
        roots: vec![
            CollectionRoot {
                id: "/work/api".into(),
                name: "api".into(),
                location: "/work/api".into(),
                collections: vec!["/work/api/orders".into(), "/work/api/users".into()],
                truncated: false,
            },
            CollectionRoot {
                id: "/work/scratch".into(),
                name: "scratch".into(),
                location: "/work/scratch".into(),
                collections: vec!["/work/scratch".into()],
                truncated: false,
            },
        ],
        collections: vec![
            entry(
                "/work/api/orders",
                "Orders",
                "/work/api",
                "orders",
                Some("/work/api"),
                2,
            ),
            entry(
                "/work/api/users",
                "Users",
                "/work/api",
                "users",
                Some("/work/api"),
                0,
            ),
            entry("/work/scratch", "Scratch", "/work/scratch", "", None, 0),
        ],
        active: Some("/work/api/orders".into()),
        repositories: vec![rt_app::git::repos::RepositoryEntry {
            id: "/work/api".into(),
            name: "api".into(),
            location: "/work/api".into(),
            root: "/work/api".into(),
            collections: vec!["/work/api/orders".into(), "/work/api/users".into()],
            linked: false,
        }],
        active_repository: Some("/work/api".into()),
    }
}

fn import_plan() -> ImportPlan {
    ImportPlan {
        id: 1,
        format: ImportFormat::Postman,
        collection_name: "My API".into(),
        folder_name: "My API".into(),
        files: 3,
        report: LossReport {
            format: ImportFormat::Postman,
            requests: 2,
            folders: 1,
            environments: 0,
            losses: vec![Loss {
                location: "Users / Get user".into(),
                kind: LossKind::Script,
                effect: LossEffect::Dropped,
                detail: "pre-request script (3 lines) not imported".into(),
            }],
        },
    }
}

fn history_entry() -> HistoryEntry {
    HistoryEntry {
        id: 7,
        at: 1_790_000_000_000,
        collection: "/home/a/api".into(),
        request_path: "users/create.yml".into(),
        name: "Create user".into(),
        environment: Some("Local".into()),
        method: "POST".into(),
        url: Some("http://127.0.0.1/users?token=***".into()),
        status: Some(201),
        error: None,
        duration_ms: Some(12.5),
        size: Some(2),
    }
}

/// `(TypeScript name, TypeScript type, value)`
fn fixtures() -> Vec<(&'static str, &'static str, serde_json::Value)> {
    let mut all = base_fixtures();
    all.extend(wave12::fixtures());
    all.extend(openapi_wizard::fixtures());
    all.extend(wave16::fixtures());
    all.extend(wave15::fixtures());
    all.extend(wave17::fixtures());
    all.extend(wave18::fixtures());
    all.extend(wave19::fixtures());
    all
}

fn base_fixtures() -> Vec<(&'static str, &'static str, serde_json::Value)> {
    fn j<T: Serialize>(v: T) -> serde_json::Value {
        serde_json::to_value(v).unwrap()
    }
    vec![
        ("requestSpec", "RequestSpec", j(request_spec())),
        ("bodies", "Body[]", j(bodies())),
        ("auths", "Auth[]", j(auths())),
        ("environment", "Environment", j(environment())),
        ("varSources", "VarSource[]", j(var_sources())),
        ("transportErrors", "TransportError[]", j(transport_errors())),
        ("sendErrors", "SendError[]", j(send_errors())),
        (
            "sendChecks",
            "SendCheck[]",
            j(vec![SendCheck {
                caller: Caller::Agent,
                purpose: Purpose::Main,
                collection: "Shop".into(),
                request_path: "orders/create.yml".into(),
                environment: Some("PRD".into()),
                method: "POST".into(),
                url: "https://api.example/orders?key=***".into(),
                effect: Effect::Write,
                production: true,
            }]),
        ),
        (
            "callers",
            "Caller[]",
            j(vec![Caller::App, Caller::Cli, Caller::Agent]),
        ),
        (
            "purposes",
            "SendPurpose[]",
            j(vec![Purpose::Main, Purpose::Script]),
        ),
        (
            "guardVerdicts",
            "GuardVerdict[]",
            j(vec![
                GuardVerdict::Allow,
                GuardVerdict::Refuse {
                    rule: "agents.production-writes".into(),
                    reason: "agents may not write to production".into(),
                },
            ]),
        ),
        ("reportOk", "SendReport", j(report_ok())),
        ("reportError", "SendReport", j(report_error())),
        (
            "flows",
            "Flow[]",
            j(vec![
                Flow::Continue,
                Flow::Skip,
                Flow::Next { request: None },
            ]),
        ),
        (
            "scriptTrust",
            "ScriptTrust",
            j(ScriptTrust {
                has_scripts: true,
                trusted: false,
            }),
        ),
        ("runPlan", "RunPlan", j(run_plan())),
        ("runEvents", "RunEvent[]", j(run_events())),
        ("binaryBody", "ReportBody", j(binary_body())),
        (
            "storedResponse",
            "StoredResponse",
            j(StoredResponse {
                report: report_ok(),
                response_id: Some(1),
                size: 2,
                line_count: 1,
                is_text: true,
                observe: true,
            }),
        ),
        (
            "lineSlice",
            "LineSlice",
            j(LineSlice {
                start: 10,
                lines: vec!["a".into(), "b".into()],
                cut: vec![1],
            }),
        ),
        ("tree", "TreeNode", j(tree())),
        (
            "preview",
            "Preview",
            j(Preview {
                url: Some("http://127.0.0.1/users?token=***".into()),
                headers: vec![("Authorization".into(), "Bearer ***".into())],
                variables: vec![
                    PreviewVar {
                        name: "baseUrl".into(),
                        value: Some("http://127.0.0.1".into()),
                        source: VarSource::Environment {
                            name: "Local".into(),
                        },
                        secret: false,
                    },
                    PreviewVar {
                        name: "token".into(),
                        value: None,
                        source: VarSource::Environment {
                            name: "Local".into(),
                        },
                        secret: true,
                    },
                ],
                unresolved: vec!["missing".into()],
                error: None,
            }),
        ),
        (
            "variableScope",
            "VariableScope",
            j(rt_app::variables::VariableScope {
                variables: vec![
                    rt_app::variables::ScopeVar {
                        name: "baseUrl".into(),
                        value: Some("http://c".into()),
                        source: VarSource::Collection,
                        secret: false,
                        external: None,
                        overridden: true,
                    },
                    rt_app::variables::ScopeVar {
                        name: "dbPassword".into(),
                        value: None,
                        source: VarSource::Environment {
                            name: "Local".into(),
                        },
                        secret: true,
                        external: Some(SecretProvider::OnePassword),
                        overridden: false,
                    },
                    rt_app::variables::ScopeVar {
                        name: "userId".into(),
                        value: Some("42".into()),
                        source: VarSource::Folder {
                            path: "users".into(),
                        },
                        secret: false,
                        external: None,
                        overridden: false,
                    },
                ],
                undefined: vec!["missing".into()],
            }),
        ),
        (
            "fileChanges",
            "FileChange[]",
            j(vec![
                FileChange {
                    path: "a.yml".into(),
                    text: "info:\n".into(),
                    deleted: false,
                },
                FileChange {
                    path: "environments/Old.yml".into(),
                    text: String::new(),
                    deleted: true,
                },
            ]),
        ),
        ("gitStatus", "GitStatus", j(git_status())),
        (
            "gitStatusNoRepo",
            "GitStatus",
            j(GitStatus {
                repo: false,
                branch: None,
                upstream: None,
                ahead: 0,
                behind: 0,
                files: vec![],
                merging: false,
            }),
        ),
        ("gitDiff", "GitDiff", j(git_diff())),
        (
            "gitBranches",
            "GitBranch[]",
            j(vec![
                GitBranch {
                    name: "main".into(),
                    current: true,
                    remote: false,
                },
                GitBranch {
                    name: "origin/main".into(),
                    current: false,
                    remote: true,
                },
            ]),
        ),
        (
            "gitCommit",
            "GitCommit",
            j(GitCommit {
                oid: "0123456789abcdef0123456789abcdef01234567".into(),
                summary: "Add users".into(),
                author: "Ada".into(),
                time: 1_790_000_000,
            }),
        ),
        (
            "gitPullResults",
            "GitPullResult[]",
            j(vec![
                GitPullResult::UpToDate,
                GitPullResult::FastForward { commits: 2 },
                GitPullResult::Merged { oid: "abc".into() },
                GitPullResult::Conflicts {
                    paths: vec!["a.yml".into()],
                },
            ]),
        ),
        (
            "gitConflict",
            "GitConflict",
            j(GitConflict {
                path: "a.yml".into(),
                base: Some("a".into()),
                ours: Some("b".into()),
                theirs: None,
            }),
        ),
        (
            "gitResolutions",
            "GitResolution[]",
            j(vec![GitResolution::Ours, GitResolution::Theirs]),
        ),
        (
            "gitIdentity",
            "GitIdentity",
            j(GitIdentity {
                name: "Ada".into(),
                email: "ada@example.com".into(),
            }),
        ),
        ("importPlan", "ImportPlan", j(import_plan())),
        // Wave 13: import into the open collection, imports converted by an extension.
        (
            "folderImport",
            "FolderImport",
            j(rt_app::import::FolderImport {
                folder: "shop".into(),
                name: "Shop".into(),
                requests: 3,
                folders: 1,
                environments_added: vec!["Staging".into()],
                environments_extended: vec!["Prod".into()],
                files: vec!["shop/recording.odtrace".into()],
                losses: import_plan().report.losses,
                contract_spec: Some("Shop/openapi.yaml".into()),
            }),
        ),
        (
            "externalImport",
            "ExternalImport",
            j(rt_app::import::ExternalImport {
                collection_name: "Trace".into(),
                files: vec![rt_app::import::ImportedFile {
                    path: "opencollection.yml".into(),
                    text: "opencollection: 1.0.0\n".into(),
                }],
                report: import_plan().report,
            }),
        ),
        (
            "importFormats",
            "ImportFormat[]",
            j(ImportFormat::ALL
                .into_iter()
                .chain([ImportFormat::Plugin])
                .collect::<Vec<_>>()),
        ),
        (
            "lossKinds",
            "LossKind[]",
            j(vec![
                LossKind::Script,
                LossKind::Test,
                LossKind::Auth,
                LossKind::Body,
                LossKind::Variable,
                LossKind::Setting,
                LossKind::Documentation,
                LossKind::Example,
                LossKind::Protocol,
                LossKind::Other,
            ]),
        ),
        (
            "lossEffects",
            "LossEffect[]",
            j(vec![LossEffect::Dropped, LossEffect::Approximated]),
        ),
        ("historyEntry", "HistoryEntry", j(history_entry())),
        (
            "historyEntryFailed",
            "HistoryEntry",
            j(HistoryEntry {
                url: None,
                status: None,
                duration_ms: None,
                size: None,
                error: Some(rt_core::send::SendError::Prepare("x".into())),
                ..history_entry()
            }),
        ),
        (
            "searchHits",
            "SearchHit[]",
            j([
                SearchField::Name,
                SearchField::Url,
                SearchField::Param,
                SearchField::Header,
                SearchField::Body,
                SearchField::Auth,
                SearchField::Variable,
            ]
            .into_iter()
            .map(|field| SearchHit {
                path: "users/create.yml".into(),
                name: "Create user".into(),
                method: "POST".into(),
                field,
                snippet: "{{baseUrl}}/users".into(),
                ranges: vec![(12, 17)],
            })
            .collect::<Vec<_>>()),
        ),
        // Wave 14: the collections in the sidebar.
        ("collectionSet", "CollectionSet", j(collection_set())),
        (
            "requestRef",
            "RequestRef",
            j(rt_app::collections::RequestRef {
                collection: "/work/api/orders".into(),
                path: "orders/create.yml".into(),
            }),
        ),
        (
            "collectionSearchHit",
            "CollectionSearchHit",
            j(rt_app::collections::CollectionSearchHit {
                collection: "/work/api/orders".into(),
                hit: SearchHit {
                    path: "orders/create.yml".into(),
                    name: "Create order".into(),
                    method: "POST".into(),
                    field: SearchField::Url,
                    snippet: "{{baseUrl}}/orders".into(),
                    ranges: vec![(12, 18)],
                },
            }),
        ),
        (
            "collectionChanges",
            "CollectionChanges[]",
            j(vec![rt_app::collections::CollectionChanges {
                collection: "/work/api/orders".into(),
                changes: vec![FileChange {
                    path: "orders/create.yml".into(),
                    text: "info:\n  name: Create order\n".into(),
                    deleted: false,
                }],
            }]),
        ),
        (
            "newCollections",
            "NewCollection[]",
            j(vec![
                rt_app::collections::NewCollection {
                    name: "Billing".into(),
                    root: Some("/work/api".into()),
                    dir: "billing".into(),
                    base_url: Some("https://billing.example.com".into()),
                    environment: Some("Local".into()),
                },
                rt_app::collections::NewCollection {
                    name: "Scratch".into(),
                    root: None,
                    dir: "scratch".into(),
                    base_url: None,
                    environment: None,
                },
            ]),
        ),
        // Wave 14: Git for any repository (graph, actions).
        ("gitGraphQuery", "GitGraphQuery", j(git_graph_query())),
        ("gitGraphPage", "GitGraphPage", j(git_graph_page())),
        (
            "gitChangedFiles",
            "GitChangedFile[]",
            j(git_changed_files()),
        ),
        ("gitActions", "GitAction[]", j(git_actions())),
        ("gitPlan", "GitPlan", j(git_plan())),
        ("gitActionResult", "GitActionResult", j(git_action_result())),
        ("gitRemotes", "GitRemote[]", j(git_remotes())),
        ("gitStashes", "GitStash[]", j(git_stashes())),
        (
            "gitBlockReasons",
            "GitBlockReason[]",
            j(git_block_reasons()),
        ),
        ("responseSides", "ResponseSides", j(response_sides())),
        ("appErrors", "AppError[]", j(app_errors())),
        (
            "environmentWithConfig",
            "Environment",
            j(environment_with_config()),
        ),
        ("secretProviders", "SecretProvider[]", j(secret_providers())),
        (
            "pluginInfo",
            "PluginInfo",
            j(PluginInfo {
                manifest: manifest(),
                granted: true,
                enabled: false,
            }),
        ),
        ("viewReply", "ViewReply", j(view_reply())),
        ("viewReplyEmpty", "ViewReply", j(ViewReply::default())),
        (
            "welcomeState",
            "WelcomeState",
            j(rt_app::plugins::WelcomeState {
                done: false,
                choices: vec![rt_app::plugins::WelcomeOffer {
                    plugin: "odata".into(),
                    choice: rt_app::plugins::WelcomeChoice {
                        label: "OData".into(),
                        description: "V2 and V4 services".into(),
                    },
                }],
            }),
        ),
        (
            "tabTargets",
            "TabTarget[]",
            j(vec![
                TabTarget {
                    request_path: "a.yml".into(),
                    response_id: Some(3),
                },
                TabTarget {
                    request_path: "a.yml".into(),
                    response_id: None,
                },
            ]),
        ),
        (
            "tokenInfo",
            "TokenInfo",
            j(TokenInfo {
                key: "k".into(),
                label: "idp app openid".into(),
                obtained_at: 1_790_000_000_000,
                expires_at: Some(1_790_000_360_000),
                refreshable: true,
            }),
        ),
        (
            "authPrompts",
            "AuthPrompt[]",
            j(vec![
                AuthPrompt::Browser {
                    url: "https://idp/auth?x".into(),
                },
                AuthPrompt::DeviceCode(DeviceCodePrompt {
                    user_code: "ABCD-EFGH".into(),
                    verification_uri: "https://idp/device".into(),
                    verification_uri_complete: None,
                    expires_in: 600,
                }),
                AuthPrompt::Done,
            ]),
        ),
        (
            "cookieInfo",
            "CookieInfo",
            j(CookieInfo {
                name: "SAP_SESSIONID".into(),
                value: "v".into(),
                domain: "example.com".into(),
                path: "/".into(),
                expires: None,
                secure: true,
                http_only: true,
                same_site: Some("Lax".into()),
            }),
        ),
        (
            "secretFinding",
            "SecretFinding",
            j(SecretFinding {
                path: "a.yml".into(),
                line: 12,
                kind: "bearer token".into(),
                snippet: "token: ***".into(),
                hint: "use a secret variable".into(),
            }),
        ),
        (
            "secretKey",
            "SecretKey",
            j(SecretKey {
                collection: "c".into(),
                environment: "Local".into(),
                variable: "token".into(),
            }),
        ),
        // Wave 8.
        ("protocols", "Protocol[]", j(protocols())),
        (
            "requestSpecGrpc",
            "RequestSpec",
            j(RequestSpec {
                method: "GRPC".into(),
                url: "grpcs://api.example:443".into(),
                protocol: protocols()[1].clone(),
                ..RequestSpec::default()
            }),
        ),
        (
            "frames",
            "Frame[]",
            j(vec![Frame::Text("a".into()), Frame::Binary(vec![255])]),
        ),
        ("streamEvents", "StreamEvent[]", j(stream_events())),
        (
            "streamLog",
            "StreamLog",
            j(StreamLog {
                version: 1,
                request_path: "ws/echo.yml".into(),
                environment: Some("Local".into()),
                kind: "websocket".into(),
                url: "wss://echo.example/ws".into(),
                events: stream_events(),
                notes: vec!["OAuth 2.0: token from cache".into()],
            }),
        ),
        (
            "grpcServices",
            "GrpcService[]",
            j(vec![GrpcService {
                name: "hello.HelloService".into(),
                methods: vec![GrpcMethod {
                    name: "SayHello".into(),
                    path: "/hello.HelloService/SayHello".into(),
                    method_type: GrpcMethodType::Unary,
                    input_type: "hello.HelloRequest".into(),
                    output_type: "hello.HelloResponse".into(),
                }],
            }]),
        ),
        (
            "grpcErrors",
            "GrpcError[]",
            j(vec![
                GrpcError::Proto("a.proto:3: syntax error".into()),
                GrpcError::Reflection("unimplemented".into()),
                GrpcError::UnknownMethod("/a.B/C".into()),
                GrpcError::Message("unknown field".into()),
            ]),
        ),
        (
            "agentPolicy",
            "AgentPolicy",
            j(AgentPolicy {
                access: AgentAccess::Read,
                environments: vec![
                    EnvironmentPolicy {
                        environment: "Local".into(),
                        access: AgentAccess::Write,
                        production: false,
                    },
                    EnvironmentPolicy {
                        environment: "PRD".into(),
                        access: AgentAccess::None,
                        production: true,
                    },
                ],
            }),
        ),
        ("effects", "Effect[]", j(vec![Effect::Read, Effect::Write])),
        (
            "verdicts",
            "Verdict[]",
            j(vec![
                Verdict::Allowed,
                Verdict::NeedsApproval,
                Verdict::Refused {
                    reason: "environment PRD allows only reading".into(),
                },
            ]),
        ),
        (
            "agentCheck",
            "AgentCheck",
            j(AgentCheck {
                effect: Effect::Write,
                verdict: Verdict::NeedsApproval,
            }),
        ),
        (
            "pendingApproval",
            "PendingApproval",
            j(PendingApproval {
                id: "a1b2".into(),
                at: 1_790_000_000_000,
                agent: "claude-code".into(),
                collection: "/work/api".into(),
                collection_name: "Orders".into(),
                request_path: "orders/create.yml".into(),
                environment: Some("PRD".into()),
                method: "POST".into(),
                url: "https://api.example/orders".into(),
                reason: "writing request in production environment PRD".into(),
            }),
        ),
        (
            "approvalDecisions",
            "ApprovalDecision[]",
            j(vec![ApprovalDecision::Approved, ApprovalDecision::Denied]),
        ),
        (
            "auditEntries",
            "AuditEntry[]",
            j(vec![
                AuditEntry {
                    at: 1_790_000_000_000,
                    agent: "claude-code".into(),
                    tool: "send_request".into(),
                    collection: "/work/api".into(),
                    request_path: Some("orders/create.yml".into()),
                    environment: Some("PRD".into()),
                    method: Some("POST".into()),
                    url: Some("https://api.example/orders".into()),
                    status: Some(201),
                    outcome: AuditOutcome::Sent,
                    approved_by: Some("app".into()),
                    rule: None,
                    detail: String::new(),
                },
                AuditEntry {
                    at: 1_790_000_000_001,
                    agent: "claude-code".into(),
                    tool: "list_requests".into(),
                    collection: "/work/api".into(),
                    request_path: None,
                    environment: None,
                    method: None,
                    url: None,
                    status: None,
                    outcome: AuditOutcome::Read,
                    approved_by: None,
                    rule: None,
                    detail: String::new(),
                },
                AuditEntry {
                    at: 1_790_000_000_002,
                    agent: "claude-code".into(),
                    tool: "send_request".into(),
                    collection: "/work/api".into(),
                    request_path: Some("orders/create.yml".into()),
                    environment: Some("PRD".into()),
                    method: Some("POST".into()),
                    url: Some("https://api.example/orders".into()),
                    status: None,
                    outcome: AuditOutcome::Refused,
                    approved_by: None,
                    rule: Some("agents.production-writes".into()),
                    detail: "agents may not write to production".into(),
                },
            ]),
        ),
        (
            "auditOutcomes",
            "AuditOutcome[]",
            j(vec![
                AuditOutcome::Read,
                AuditOutcome::Sent,
                AuditOutcome::Refused,
                AuditOutcome::Denied,
                AuditOutcome::TimedOut,
                AuditOutcome::Failed,
            ]),
        ),
        (
            "aiConfigs",
            "AiConfig[]",
            j(vec![
                AiConfig {
                    provider: AiProviderKind::Anthropic,
                    model: "claude-opus-5-5".into(),
                    base_url: None,
                },
                AiConfig {
                    provider: AiProviderKind::OpenaiCompatible,
                    model: "llama3.1".into(),
                    base_url: Some("http://localhost:11434/v1".into()),
                },
            ]),
        ),
        (
            "aiTasks",
            "AiTask[]",
            j(vec![
                AiTask::CreateRequest {
                    description: "create a user".into(),
                    folder: "users".into(),
                },
                AiTask::ExplainError {
                    path: "a.yml".into(),
                    response: 3,
                },
                AiTask::SuggestTests {
                    path: "a.yml".into(),
                    response: 3,
                },
            ]),
        ),
        (
            "aiAnswer",
            "AiAnswer",
            j(AiAnswer {
                text: "The server wants `email`.".into(),
                request: Some(request_spec()),
                scripts: vec![Script {
                    phase: ScriptPhase::Tests,
                    code: "test('201', () => expect(rt.response.status).to.equal(201));".into(),
                }],
                assertions: Vec::new(),
                ..Default::default()
            }),
        ),
        (
            "aiErrors",
            "AiError[]",
            j(vec![
                AiError::NotConfigured,
                AiError::NoKey,
                AiError::Transport("connection refused".into()),
                AiError::Provider("401 invalid x-api-key".into()),
                AiError::Answer("no request in the answer".into()),
            ]),
        ),
        (
            "mcpServerConfigs",
            "McpServerConfig[]",
            j(vec![
                mcp_test().server,
                McpServerConfig::Http {
                    url: "http://127.0.0.1:3000/mcp".into(),
                    headers: vec![("Authorization".into(), "Bearer {{token}}".into())],
                },
            ]),
        ),
        (
            "mcpServerInfo",
            "McpServerInfo",
            j(McpServerInfo {
                name: "orders".into(),
                version: "1.0.0".into(),
                protocol_version: "2025-06-18".into(),
                capabilities: vec!["tools".into()],
                instructions: None,
            }),
        ),
        (
            "mcpTool",
            "McpTool",
            j(McpTool {
                name: "list_orders".into(),
                title: Some("List orders".into()),
                description: None,
                input_schema: serde_json::json!({"type": "object"}),
                output_schema: None,
                annotations: Some(serde_json::json!({"readOnlyHint": true})),
            }),
        ),
        ("mcpCallResult", "McpCallResult", j(mcp_result())),
        (
            "mcpCallResultWithNotifications",
            "McpCallResult",
            j(McpCallResult {
                notifications: vec![McpNotification {
                    method: "notifications/progress".into(),
                    params: serde_json::json!({"progressToken": 3, "progress": 1, "total": 2}),
                }],
                ..mcp_result()
            }),
        ),
        (
            "mcpResources",
            "McpResource[]",
            j(vec![
                McpResource {
                    uri: "demo://readme".into(),
                    name: "README".into(),
                    title: Some("About".into()),
                    description: Some("What it offers".into()),
                    mime_type: Some("text/markdown".into()),
                    size: Some(120),
                },
                McpResource {
                    uri: "file:///a".into(),
                    name: "a".into(),
                    title: None,
                    description: None,
                    mime_type: None,
                    size: None,
                },
            ]),
        ),
        (
            "mcpResourceContents",
            "McpResourceContents[]",
            j(vec![
                McpResourceContents {
                    uri: "demo://readme".into(),
                    mime_type: Some("text/markdown".into()),
                    text: Some("# Hi".into()),
                    blob: None,
                },
                McpResourceContents {
                    uri: "demo://logo".into(),
                    mime_type: None,
                    text: None,
                    blob: Some("iVBORw0K".into()),
                },
            ]),
        ),
        (
            "mcpPrompts",
            "McpPrompt[]",
            j(vec![McpPrompt {
                name: "summarize".into(),
                title: None,
                description: Some("Summarize".into()),
                arguments: vec![McpPromptArgument {
                    name: "text".into(),
                    title: Some("Text".into()),
                    description: None,
                    required: true,
                }],
            }]),
        ),
        (
            "mcpSelfServers",
            "McpSelfServer[]",
            j(vec![
                McpSelfServer {
                    server: Some(McpServerConfig::Stdio {
                        command: "rt".into(),
                        args: vec!["mcp".into()],
                        env: Vec::new(),
                        cwd: None,
                    }),
                    hint: "Starts rt mcp on this collection.".into(),
                },
                McpSelfServer {
                    server: None,
                    hint: "Install rt first.".into(),
                },
            ]),
        ),
        (
            "mcpPromptResult",
            "McpPromptResult",
            j(McpPromptResult {
                description: None,
                messages: vec![McpPromptMessage {
                    role: "user".into(),
                    content: McpContent::Text {
                        text: "Summarize".into(),
                    },
                }],
            }),
        ),
        ("mcpTest", "McpTest", j(mcp_test())),
        (
            "mcpTestResult",
            "McpTestResult",
            j(McpTestResult {
                path: "mcp/lists-orders.mcp.yaml".into(),
                passed: false,
                result: Some(mcp_result()),
                error: None,
                assertions: Vec::new(),
            }),
        ),
    ]
}

/// Wave 16 interface: providers and profiles, tool use, streaming, the generic call of
/// extensions, the preview, the opt-out and the changelog facts.
/// Wave 15 interface: the Git client for any repository (repositories in the set, working copy,
/// file tree and blobs, blame, history search and reflog, building blocks for history editors,
/// the index, signatures, new pull requests).
mod wave15 {
    use rt_app::git::files::{GitBlob, GitEntryKind, GitTreeEntry, GitWorkingCopy, GitWorkingFile};
    use rt_app::git::graph::GitGraphCommit;
    use rt_app::git::history::{
        GitBlame, GitBlameHunk, GitHistoryField, GitHistoryHit, GitHistoryPage, GitHistoryQuery,
        GitReflogEntry,
    };
    use rt_app::git::index::{
        GitHunkSelection, GitIndexDiff, GitPatchRequest, GitPatchSide, GitStagedCommit,
    };
    use rt_app::git::plumbing::{
        GitCommitDetails, GitCommitWrite, GitPickRequest, GitPickResult, GitSignature,
        GitTreeContent, GitTreeFile, GitTreeWrite,
    };
    use rt_app::git::repos::{GitCloneRequest, GitOperation, GitRepoSummary, RepositoryEntry};
    use rt_app::git::signing::{GitSignatureCheck, GitSignatureKind, GitSignatureState};
    use rt_app::git::{
        DiffHunk, DiffLine, DiffLineKind, GitCommit, GitConflict, GitDiff, GitFileChange,
        GitFileState,
    };
    use rt_app::review::NewPullRequest;
    use serde::Serialize;

    fn j<T: Serialize>(v: T) -> serde_json::Value {
        serde_json::to_value(v).unwrap()
    }

    fn commit(c: &str, summary: &str) -> GitCommit {
        GitCommit {
            oid: c.repeat(40),
            summary: summary.into(),
            author: "Ada".into(),
            time: 1_700_000_000,
        }
    }

    fn signature() -> GitSignature {
        GitSignature {
            name: "Ada".into(),
            email: "ada@example.com".into(),
            time: 1_700_000_000,
            offset: 60,
        }
    }

    fn diff() -> GitDiff {
        GitDiff {
            path: "/src/total.rs".into(),
            binary: false,
            hunks: vec![DiffHunk {
                old_start: 3,
                old_lines: 1,
                new_start: 3,
                new_lines: 2,
                lines: vec![
                    DiffLine {
                        kind: DiffLineKind::Removed,
                        text: "let total = 0;".into(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Added,
                        text: "let total = sum(items);".into(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Context,
                        text: "total".into(),
                    },
                ],
            }],
        }
    }

    fn patch(side: GitPatchSide) -> GitPatchRequest {
        GitPatchRequest {
            path: "/src/total.rs".into(),
            side,
            old: "1".repeat(40),
            new: "2".repeat(40),
            hunks: vec![
                GitHunkSelection {
                    hunk: 0,
                    lines: Some(vec![1]),
                },
                GitHunkSelection {
                    hunk: 1,
                    lines: None,
                },
            ],
        }
    }

    pub fn fixtures() -> Vec<(&'static str, &'static str, serde_json::Value)> {
        vec![
            (
                "repositoryRoot",
                "RepositoryEntry",
                j(RepositoryEntry {
                    id: "/work/website".into(),
                    name: "website".into(),
                    location: "/work/website".into(),
                    root: "/work/website".into(),
                    collections: Vec::new(),
                    linked: true,
                }),
            ),
            (
                "gitRepoSummaries",
                "GitRepoSummary[]",
                j([
                    GitRepoSummary {
                        id: "/work/api".into(),
                        branch: Some("main".into()),
                        upstream: Some("origin/main".into()),
                        ahead: 1,
                        behind: 2,
                        changes: 3,
                        operation: None,
                    },
                    GitRepoSummary {
                        id: "/work/website".into(),
                        branch: None,
                        upstream: None,
                        ahead: 0,
                        behind: 0,
                        changes: 0,
                        operation: Some(GitOperation::Rebase),
                    },
                ]),
            ),
            (
                "gitOperations",
                "GitOperation[]",
                j([
                    GitOperation::Merge,
                    GitOperation::CherryPick,
                    GitOperation::Revert,
                    GitOperation::Rebase,
                ]),
            ),
            (
                "gitCloneRequests",
                "GitCloneRequest[]",
                j([
                    GitCloneRequest {
                        url: "https://github.com/acme/web.git".into(),
                        parent: Some("/work".into()),
                        name: Some("web".into()),
                        branch: Some("develop".into()),
                    },
                    GitCloneRequest {
                        url: "https://github.com/acme/api.git".into(),
                        parent: None,
                        name: None,
                        branch: None,
                    },
                ]),
            ),
            (
                "gitWorkingCopy",
                "GitWorkingCopy",
                j(GitWorkingCopy {
                    repository: "/work/api".into(),
                    branch: Some("main".into()),
                    upstream: Some("origin/main".into()),
                    ahead: 0,
                    behind: 1,
                    head: Some("c".repeat(40)),
                    detached: false,
                    operation: Some(GitOperation::Merge),
                    files: vec![
                        GitWorkingFile {
                            path: "/orders/create.yml".into(),
                            state: GitFileState::Modified,
                            from: None,
                            kind: GitEntryKind::File,
                            staged: Some(GitFileChange::Modified),
                            unstaged: true,
                            collection: Some("/work/api/orders".into()),
                        },
                        GitWorkingFile {
                            path: "/docs/new-name.md".into(),
                            state: GitFileState::Modified,
                            from: Some("/docs/old-name.md".into()),
                            kind: GitEntryKind::File,
                            staged: Some(GitFileChange::Renamed),
                            unstaged: false,
                            collection: None,
                        },
                        GitWorkingFile {
                            path: "/vendor/lib".into(),
                            state: GitFileState::Modified,
                            from: None,
                            kind: GitEntryKind::Submodule,
                            staged: None,
                            unstaged: true,
                            collection: None,
                        },
                        GitWorkingFile {
                            path: "/notes.txt".into(),
                            state: GitFileState::Untracked,
                            from: None,
                            kind: GitEntryKind::File,
                            staged: None,
                            unstaged: true,
                            collection: None,
                        },
                    ],
                    truncated: false,
                }),
            ),
            (
                "gitTreeEntries",
                "GitTreeEntry[]",
                j([
                    GitTreeEntry {
                        name: "src".into(),
                        path: "/src".into(),
                        kind: GitEntryKind::Dir,
                        size: None,
                        state: None,
                        ignored: false,
                    },
                    GitTreeEntry {
                        name: "logo.png".into(),
                        path: "/logo.png".into(),
                        kind: GitEntryKind::File,
                        size: Some(2048),
                        state: Some(GitFileState::Modified),
                        ignored: false,
                    },
                    GitTreeEntry {
                        name: "link".into(),
                        path: "/link".into(),
                        kind: GitEntryKind::Symlink,
                        size: Some(7),
                        state: None,
                        ignored: false,
                    },
                    GitTreeEntry {
                        name: "target".into(),
                        path: "/target".into(),
                        kind: GitEntryKind::Dir,
                        size: None,
                        state: None,
                        ignored: true,
                    },
                ]),
            ),
            (
                "gitBlobs",
                "GitBlob[]",
                j([
                    GitBlob {
                        path: "/README.md".into(),
                        rev: Some("HEAD".into()),
                        oid: Some("3".repeat(40)),
                        size: 6,
                        binary: false,
                        media_type: None,
                        text: Some("Hello\n".into()),
                        base64: None,
                        truncated: false,
                        lfs_pointer: false,
                    },
                    GitBlob {
                        path: "/logo.png".into(),
                        rev: None,
                        oid: None,
                        size: 4,
                        binary: true,
                        media_type: Some("image/png".into()),
                        text: None,
                        base64: Some("iVBORw==".into()),
                        truncated: false,
                        lfs_pointer: false,
                    },
                    GitBlob {
                        path: "/video.mp4".into(),
                        rev: Some("HEAD".into()),
                        oid: Some("4".repeat(40)),
                        size: 130,
                        binary: false,
                        media_type: None,
                        text: None,
                        base64: None,
                        truncated: true,
                        lfs_pointer: true,
                    },
                ]),
            ),
            (
                "gitBlame",
                "GitBlame",
                j(GitBlame {
                    path: "/src/total.rs".into(),
                    rev: "c".repeat(40),
                    lines: 12,
                    hunks: vec![
                        GitBlameHunk {
                            start: 1,
                            lines: 10,
                            commit: commit("a", "Initial"),
                            path: "/src/sum.rs".into(),
                            orig_start: 1,
                        },
                        GitBlameHunk {
                            start: 11,
                            lines: 2,
                            commit: commit("c", "Fix the total"),
                            path: "/src/total.rs".into(),
                            orig_start: 11,
                        },
                    ],
                    binary: false,
                }),
            ),
            (
                "gitHistoryQueries",
                "GitHistoryQuery[]",
                j([
                    GitHistoryQuery {
                        message: Some("fix".into()),
                        author: Some("ada".into()),
                        path: Some("/src".into()),
                        content: Some("total".into()),
                        since: Some(1_690_000_000),
                        until: Some(1_700_000_000),
                        refs: Some(vec!["refs/heads/main".into()]),
                        skip: 50,
                        limit: 50,
                    },
                    GitHistoryQuery {
                        limit: 100,
                        ..Default::default()
                    },
                ]),
            ),
            (
                "gitHistoryPage",
                "GitHistoryPage",
                j(GitHistoryPage {
                    hits: vec![GitHistoryHit {
                        commit: GitGraphCommit {
                            commit: commit("c", "Fix the total"),
                            parents: vec!["a".repeat(40)],
                            email: "ada@example.com".into(),
                            refs: vec!["refs/heads/main".into()],
                            signed: false,
                        },
                        matched: vec![
                            GitHistoryField::Message,
                            GitHistoryField::Author,
                            GitHistoryField::Path,
                            GitHistoryField::Content,
                        ],
                        paths: vec!["/src/total.rs".into()],
                    }],
                    more: true,
                    scanned: 2000,
                }),
            ),
            (
                "gitReflog",
                "GitReflogEntry[]",
                j([
                    GitReflogEntry {
                        name: "HEAD".into(),
                        index: 0,
                        old: Some("a".repeat(40)),
                        new: "c".repeat(40),
                        message: "commit: Fix the total".into(),
                        time: 1_700_000_000,
                        commit: Some(commit("c", "Fix the total")),
                    },
                    GitReflogEntry {
                        name: "refs/heads/main".into(),
                        index: 1,
                        old: None,
                        new: "a".repeat(40),
                        message: "branch: Created from HEAD".into(),
                        time: 1_690_000_000,
                        commit: None,
                    },
                ]),
            ),
            (
                "gitCommitDetails",
                "GitCommitDetails",
                j(GitCommitDetails {
                    oid: "c".repeat(40),
                    tree: "5".repeat(40),
                    parents: vec!["a".repeat(40)],
                    message: "Fix the total\n\nSums the items.\n".into(),
                    author: signature(),
                    committer: signature(),
                    signed: true,
                }),
            ),
            (
                "gitPickRequests",
                "GitPickRequest[]",
                j([
                    GitPickRequest {
                        onto: "HEAD".into(),
                        commit: "c".repeat(40),
                        revert: false,
                        mainline: None,
                    },
                    GitPickRequest {
                        onto: "main".into(),
                        commit: "d".repeat(40),
                        revert: true,
                        mainline: Some(1),
                    },
                ]),
            ),
            (
                "gitPickResult",
                "GitPickResult",
                j(GitPickResult {
                    tree: "6".repeat(40),
                    clean: false,
                    conflicts: vec![GitConflict {
                        path: "/src/total.rs".into(),
                        base: Some("a\n".into()),
                        ours: Some("b\n".into()),
                        theirs: None,
                    }],
                    other_conflicts: vec!["/logo.png".into()],
                }),
            ),
            (
                "gitTreeWrite",
                "GitTreeWrite",
                j(GitTreeWrite {
                    base: "6".repeat(40),
                    files: vec![
                        GitTreeFile {
                            path: "/src/total.rs".into(),
                            content: GitTreeContent::Text { text: "c\n".into() },
                            executable: None,
                        },
                        GitTreeFile {
                            path: "/logo.png".into(),
                            content: GitTreeContent::Base64 {
                                data: "iVBORw==".into(),
                            },
                            executable: None,
                        },
                        GitTreeFile {
                            path: "/build.sh".into(),
                            content: GitTreeContent::From {
                                rev: "d".repeat(40),
                            },
                            executable: Some(true),
                        },
                        GitTreeFile {
                            path: "/old.txt".into(),
                            content: GitTreeContent::Delete,
                            executable: None,
                        },
                    ],
                }),
            ),
            (
                "gitCommitWrites",
                "GitCommitWrite[]",
                j([
                    GitCommitWrite {
                        tree: "7".repeat(40),
                        parents: vec!["a".repeat(40)],
                        message: "Squashed".into(),
                        author: Some(signature()),
                        committer: None,
                    },
                    GitCommitWrite {
                        tree: "7".repeat(40),
                        parents: Vec::new(),
                        message: "Root".into(),
                        author: None,
                        committer: Some(signature()),
                    },
                ]),
            ),
            (
                "gitIndexDiff",
                "GitIndexDiff",
                j(GitIndexDiff {
                    side: GitPatchSide::Unstaged,
                    diff: diff(),
                    old: "1".repeat(40),
                    new: "2".repeat(40),
                }),
            ),
            (
                "gitPatchRequests",
                "GitPatchRequest[]",
                j([patch(GitPatchSide::Unstaged), patch(GitPatchSide::Staged)]),
            ),
            (
                "gitStagedCommit",
                "GitStagedCommit",
                j(GitStagedCommit {
                    message: "Stage only the fix".into(),
                }),
            ),
            (
                "gitSignatureChecks",
                "GitSignatureCheck[]",
                j([
                    GitSignatureCheck {
                        oid: "c".repeat(40),
                        state: GitSignatureState::Good,
                        kind: Some(GitSignatureKind::Ssh),
                        signer: Some("ada@example.com".into()),
                        key: Some("SHA256:abc".into()),
                    },
                    GitSignatureCheck {
                        oid: "a".repeat(40),
                        state: GitSignatureState::Unsigned,
                        kind: None,
                        signer: None,
                        key: None,
                    },
                ]),
            ),
            (
                "gitSignatureStates",
                "GitSignatureState[]",
                j([
                    GitSignatureState::Unsigned,
                    GitSignatureState::Unverified,
                    GitSignatureState::Good,
                    GitSignatureState::Untrusted,
                    GitSignatureState::Bad,
                    GitSignatureState::UnknownKey,
                    GitSignatureState::Expired,
                ]),
            ),
            (
                "gitSignatureKinds",
                "GitSignatureKind[]",
                j([
                    GitSignatureKind::Gpg,
                    GitSignatureKind::Ssh,
                    GitSignatureKind::X509,
                ]),
            ),
            (
                "newPullRequest",
                "NewPullRequest",
                j(NewPullRequest {
                    title: "Login with passkeys".into(),
                    body: "Adds passkeys.".into(),
                    head: "feature/login".into(),
                    base: "main".into(),
                    draft: true,
                }),
            ),
        ]
    }
}

mod wave16 {
    use rt_app::ai::{
        AiAnswer, AiCollectionPolicy, AiDelta, AiError, AiEvent, AiMessage, AiNameTarget,
        AiPreview, AiPreviewMode, AiProfile, AiProviderKind, AiReply, AiRequest, AiRole, AiRoles,
        AiScope, AiSettings, AiStop, AiTask, AiTool, AiToolCall, AiToolResult, AiUsage,
    };
    use rt_app::contract::{ChangelogCommit, ChangelogFacts};
    use serde::Serialize;
    use serde_json::json;

    fn j<T: Serialize>(v: T) -> serde_json::Value {
        serde_json::to_value(v).unwrap()
    }

    fn tool_call() -> AiToolCall {
        AiToolCall {
            id: "toolu_1".into(),
            name: "get_request".into(),
            input: json!({ "path": "users/get.yml" }),
        }
    }

    fn messages() -> Vec<AiMessage> {
        vec![
            AiMessage::text(AiRole::User, "Why does this fail?"),
            AiMessage {
                role: AiRole::Assistant,
                content: "Let me read the request.".into(),
                tool_calls: vec![tool_call()],
                tool_results: Vec::new(),
            },
            AiMessage {
                role: AiRole::User,
                content: String::new(),
                tool_calls: Vec::new(),
                tool_results: vec![AiToolResult {
                    call_id: "toolu_1".into(),
                    content: "{\"method\":\"GET\"}".into(),
                    is_error: false,
                }],
            },
        ]
    }

    fn reply() -> AiReply {
        AiReply {
            text: "Add the CSRF token fetch.".into(),
            tool_calls: vec![tool_call()],
            stop: AiStop::ToolUse,
            usage: Some(AiUsage {
                input_tokens: 1200,
                output_tokens: 80,
            }),
        }
    }

    pub fn fixtures() -> Vec<(&'static str, &'static str, serde_json::Value)> {
        let profile = |id: &str, provider, model: &str, base: Option<&str>| AiProfile {
            id: id.into(),
            label: id.into(),
            config: rt_app::ai::AiConfig {
                provider,
                model: model.into(),
                base_url: base.map(Into::into),
            },
        };
        vec![
            (
                "aiProviderKinds",
                "AiProviderKind[]",
                j(AiProviderKind::ALL),
            ),
            (
                "aiCapabilities",
                "AiCapabilities[]",
                j(AiProviderKind::ALL
                    .iter()
                    .map(|k| k.capabilities())
                    .collect::<Vec<_>>()),
            ),
            (
                "aiSettings",
                "AiSettings",
                j(AiSettings {
                    profiles: vec![
                        profile("smart", AiProviderKind::Anthropic, "claude-opus-5-5", None),
                        profile(
                            "azure",
                            AiProviderKind::AzureOpenai,
                            "gpt-deployment",
                            Some("https://acme.openai.azure.com/openai/v1"),
                        ),
                        profile(
                            "local",
                            AiProviderKind::OpenaiCompatible,
                            "llama3.1",
                            Some("http://localhost:11434/v1"),
                        ),
                    ],
                    default_profile: Some("smart".into()),
                    roles: AiRoles {
                        fast: Some("local".into()),
                        smart: Some("smart".into()),
                    },
                    preview: AiPreviewMode::FirstUse,
                    previewed: vec!["smart".into()],
                }),
            ),
            ("aiSettingsEmpty", "AiSettings", j(AiSettings::default())),
            (
                "aiCollectionPolicy",
                "AiCollectionPolicy",
                j(AiCollectionPolicy {
                    file_off: false,
                    device_off: true,
                }),
            ),
            (
                "aiTasksWave16",
                "AiTask[]",
                j(vec![
                    AiTask::RequestFromSnippet {
                        snippet: "fetch('https://api.example.com/users')".into(),
                        folder: "users".into(),
                    },
                    AiTask::ExplainResponse {
                        path: "users/get.yml".into(),
                        response: 4,
                    },
                    AiTask::SuggestAssertions {
                        path: "users/get.yml".into(),
                        response: 4,
                    },
                    AiTask::SuggestNames {
                        target: AiNameTarget::Variable,
                        path: "users/get.yml".into(),
                    },
                    AiTask::DraftDocs {
                        path: "users/get.yml".into(),
                    },
                ]),
            ),
            (
                "aiAnswerWave16",
                "AiAnswer",
                j(AiAnswer {
                    text: "Names for the request.".into(),
                    names: vec!["List users".into(), "Get users".into()],
                    docs: Some("Returns the users of a tenant.".into()),
                    ..Default::default()
                }),
            ),
            (
                "aiErrorsWave16",
                "AiError[]",
                j(vec![
                    AiError::Unsupported("tool use with this provider".into()),
                    AiError::Cancelled,
                    AiError::OptedOut,
                ]),
            ),
            (
                "aiRequest",
                "AiRequest",
                j(AiRequest {
                    id: "call-1".into(),
                    purpose: "Explain this failure".into(),
                    profile: Some("smart".into()),
                    system: "You explain failed HTTP requests.".into(),
                    messages: messages(),
                    scope: AiScope {
                        request: Some("users/get.yml".into()),
                        response: Some(4),
                        folder: None,
                    },
                    tools: vec![AiTool {
                        name: "get_request".into(),
                        description: "Reads a request of the collection.".into(),
                        input_schema: json!({
                            "type": "object",
                            "properties": { "path": { "type": "string" } },
                            "required": ["path"]
                        }),
                    }],
                    json: Some(json!({ "type": "object" })),
                    max_tokens: Some(4000),
                }),
            ),
            (
                "aiRequestMinimal",
                "AiRequest",
                j(AiRequest {
                    id: "call-2".into(),
                    purpose: "Name this request".into(),
                    profile: None,
                    system: String::new(),
                    messages: vec![AiMessage::text(AiRole::User, "GET /users")],
                    scope: AiScope::default(),
                    tools: Vec::new(),
                    json: None,
                    max_tokens: None,
                }),
            ),
            (
                "aiPreview",
                "AiPreview",
                j(AiPreview {
                    profile: "smart".into(),
                    provider: AiProviderKind::Anthropic,
                    model: "claude-opus-5-5".into(),
                    endpoint: "https://api.anthropic.com/v1/messages".into(),
                    call: rt_app::ai::AiCall {
                        system: "You explain failed HTTP requests.".into(),
                        messages: messages(),
                        max_tokens: 4000,
                        ..Default::default()
                    },
                    masked: 2,
                    bytes: 1834,
                    required: true,
                }),
            ),
            (
                "aiEvents",
                "AiEvent[]",
                j(vec![
                    AiEvent::Delta {
                        delta: AiDelta::Text {
                            text: "Add the".into(),
                        },
                    },
                    AiEvent::Delta {
                        delta: AiDelta::ToolCall { call: tool_call() },
                    },
                    AiEvent::Done { reply: reply() },
                    AiEvent::Done {
                        reply: AiReply::default(),
                    },
                    AiEvent::Error {
                        error: AiError::Cancelled,
                    },
                ]),
            ),
            (
                "changelogFacts",
                "ChangelogFacts",
                j(ChangelogFacts {
                    from: "v1.0.0".into(),
                    to: "v1.1.0".into(),
                    title: "Orders API 1.0.0 → 1.1.0".into(),
                    spec: Default::default(),
                    collection: Default::default(),
                    sections: vec![
                        "## Service metadata\n\n- Entity `Order`: new property `Note`".into(),
                    ],
                    commits: vec![
                        ChangelogCommit {
                            oid: "b".repeat(40),
                            summary: "Merge pull request #12 from acme/notes".into(),
                            body: "Orders get a note".into(),
                            author: "Ada".into(),
                            pull: Some(12),
                        },
                        ChangelogCommit {
                            oid: "c".repeat(40),
                            summary: "Fix typo".into(),
                            body: String::new(),
                            author: "Ada".into(),
                            pull: None,
                        },
                    ],
                    markdown: "# Orders API 1.0.0 → 1.1.0\n".into(),
                }),
            ),
        ]
    }
}

/// Wave 12 (Git for APIs): revisions, semantic diff and merge, compare and bisect, timeline,
/// review, contract, rescue.
mod wave12 {
    use super::{report_ok, request_spec};
    use rt_app::bisect::{BisectOutcome, BisectRequest, BisectStep, BisectVerdict};
    use rt_app::compare::{
        BodyChange, CompareRequest, HeaderChange, RequestComparison, RevisionRun,
        RevisionRunRequest, RunComparison, TestChange, ValueChange,
    };
    use rt_app::contract::{
        ContractFinding, ContractReport, ContractSpec, FindingKind, SpecChange, SpecChangeKind,
        SpecChanges,
    };
    use rt_app::diffs::{
        ChangeKind, ChangeTarget, ChangedFile, CollectionDiff, DiffScope, DiffValue, FieldChange,
        FileStatus, ModelChange,
    };
    use rt_app::git::{GitCommit, GitFileChange, GitFileCommit, GitTree};
    use rt_app::merges::{ConflictChoice, FieldConflict, MergeState, Side};
    use rt_app::rescue::{
        HistoryFinding, HistoryScan, RescuePlan, RescueResult, RewrittenBranch, ScanProgress,
    };
    use rt_app::review::{
        CiCheck, CiState, CiStatus, NewReviewComment, PullRequest, PullState, ReviewComment,
        ReviewError, ReviewHost, ReviewKind, ReviewRepo, ReviewSession, ReviewSubmission,
        ReviewVerdict,
    };
    use rt_app::secret_scan::SecretFinding;
    use rt_app::timeline::{
        FieldBlame, RecordSource, RecordedTest, RequestTimeline, TestRecord, TimelineEntry,
    };
    use rt_core::runner::{RunStep, RunSummary};
    use serde::Serialize;

    fn j<T: Serialize>(v: T) -> serde_json::Value {
        serde_json::to_value(v).unwrap()
    }

    fn path(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| (*s).to_owned()).collect()
    }

    fn text(t: &str) -> Option<DiffValue> {
        Some(DiffValue::Text { text: t.into() })
    }

    fn commit(oid: &str, summary: &str) -> GitCommit {
        GitCommit {
            oid: oid.repeat(40 / oid.len()),
            summary: summary.into(),
            author: "Ada".into(),
            time: 1_795_000_000,
        }
    }

    fn summary() -> RunSummary {
        RunSummary {
            requests: 1,
            passed: 1,
            failed: 0,
            skipped: 0,
            tests_passed: 1,
            tests_failed: 0,
            assertions_passed: 1,
            assertions_failed: 0,
            duration_ms: 20,
            stopped: false,
        }
    }

    fn field_change() -> FieldChange {
        FieldChange {
            path: path(&["url"]),
            old: text("{{baseUrl}}/v1/users"),
            new: text("{{baseUrl}}/v2/users"),
            line: Some(4),
        }
    }

    fn collection_diff() -> CollectionDiff {
        CollectionDiff {
            changes: vec![
                ModelChange {
                    target: ChangeTarget::Request {
                        path: "users/get.yml".into(),
                    },
                    change: ChangeKind::Modified,
                    file: "users/get.yml".into(),
                    name: "Get user".into(),
                    fields: vec![
                        field_change(),
                        FieldChange {
                            path: path(&["headers", "Authorization"]),
                            old: None,
                            new: text("Bearer {{token}}"),
                            line: None,
                        },
                        FieldChange {
                            path: path(&["auth"]),
                            old: Some(DiffValue::Json {
                                value: serde_json::json!({"type": "none"}),
                            }),
                            new: None,
                            line: Some(9),
                        },
                    ],
                },
                ModelChange {
                    target: ChangeTarget::Request {
                        path: "users/list.yml".into(),
                    },
                    change: ChangeKind::Moved {
                        from: "list.yml".into(),
                    },
                    file: "users/list.yml".into(),
                    name: "List users".into(),
                    fields: Vec::new(),
                },
                ModelChange {
                    target: ChangeTarget::Folder {
                        path: "admin".into(),
                    },
                    change: ChangeKind::Added,
                    file: "admin/folder.yml".into(),
                    name: "admin".into(),
                    fields: Vec::new(),
                },
                ModelChange {
                    target: ChangeTarget::Collection,
                    change: ChangeKind::Removed,
                    file: "opencollection.yml".into(),
                    name: "Demo".into(),
                    fields: Vec::new(),
                },
                ModelChange {
                    target: ChangeTarget::Environment { name: "PRD".into() },
                    change: ChangeKind::Modified,
                    file: "environments/PRD.yml".into(),
                    name: "PRD".into(),
                    fields: vec![FieldChange {
                        path: path(&["variables", "apiKey"]),
                        old: Some(DiffValue::Secret),
                        new: Some(DiffValue::Secret),
                        line: None,
                    }],
                },
            ],
            files: vec![
                ChangedFile {
                    path: "users/get.yml".into(),
                    status: FileStatus::Modified,
                    covered: true,
                },
                ChangedFile {
                    path: "admin/folder.yml".into(),
                    status: FileStatus::Added,
                    covered: true,
                },
                ChangedFile {
                    path: "list.yml".into(),
                    status: FileStatus::Removed,
                    covered: false,
                },
            ],
        }
    }

    fn field_conflict() -> FieldConflict {
        FieldConflict {
            target: ChangeTarget::Request {
                path: "users/get.yml".into(),
            },
            file: "users/get.yml".into(),
            field: path(&["url"]),
            base: text("/v1"),
            ours: text("/v2"),
            theirs: None,
        }
    }

    fn run_step() -> RunStep {
        RunStep {
            iteration: 0,
            request_path: "users/get.yml".into(),
            request_name: "Get user".into(),
            report: report_ok(),
            passed: true,
            skipped: false,
        }
    }

    fn pull() -> PullRequest {
        PullRequest {
            number: 7,
            title: "Users v2".into(),
            author: "grace".into(),
            state: PullState::Open,
            draft: false,
            source: "users-v2".into(),
            target: "main".into(),
            head: "b".repeat(40),
            base: "a".repeat(40),
            url: "https://github.com/acme/orders/pull/7".into(),
            updated: 1_795_000_000,
        }
    }

    fn test_record() -> TestRecord {
        TestRecord {
            collection: "".into(),
            request: "users/get.yml".into(),
            environment: Some("Local".into()),
            time: 1_795_000_000,
            source: RecordSource::Runner,
            passed: false,
            status: Some(200),
            tests: vec![RecordedTest {
                name: "status is 200".into(),
                passed: true,
            }],
            duration_ms: None,
        }
    }

    pub fn fixtures() -> Vec<(&'static str, &'static str, serde_json::Value)> {
        vec![
            (
                "gitTree",
                "GitTree",
                j(GitTree {
                    oid: "a".repeat(40),
                    files: [(
                        "opencollection.yml".to_string(),
                        "opencollection: 1.0.0\n".to_string(),
                    )]
                    .into_iter()
                    .collect(),
                }),
            ),
            (
                "gitFileCommits",
                "GitFileCommit[]",
                j(vec![
                    GitFileCommit {
                        commit: commit("b", "Rename"),
                        path: "users/get.yml".into(),
                        change: GitFileChange::Renamed,
                        from: Some("get.yml".into()),
                    },
                    GitFileCommit {
                        commit: commit("a", "Add"),
                        path: "get.yml".into(),
                        change: GitFileChange::Added,
                        from: None,
                    },
                    GitFileCommit {
                        commit: commit("c", "Change"),
                        path: "get.yml".into(),
                        change: GitFileChange::Modified,
                        from: None,
                    },
                    GitFileCommit {
                        commit: commit("d", "Drop"),
                        path: "get.yml".into(),
                        change: GitFileChange::Deleted,
                        from: None,
                    },
                ]),
            ),
            ("collectionDiff", "CollectionDiff", j(collection_diff())),
            (
                "diffScopes",
                "DiffScope[]",
                j(vec![
                    DiffScope::Working,
                    DiffScope::Commit { rev: "HEAD".into() },
                    DiffScope::Between {
                        from: "main".into(),
                        to: "users-v2".into(),
                    },
                ]),
            ),
            (
                "mergeState",
                "MergeState",
                j(MergeState {
                    merging: true,
                    conflicts: vec![field_conflict()],
                    text_conflicts: vec!["notes.yml".into()],
                    clean: vec!["users/list.yml".into()],
                }),
            ),
            (
                "conflictChoices",
                "ConflictChoice[]",
                j(vec![
                    ConflictChoice::Field {
                        target: field_conflict().target,
                        field: path(&["url"]),
                        side: Side::Theirs,
                    },
                    ConflictChoice::File {
                        path: "notes.yml".into(),
                        side: Side::Ours,
                    },
                ]),
            ),
            (
                "revisionRunRequest",
                "RevisionRunRequest",
                j(RevisionRunRequest {
                    rev: "refs/requesttailor/review/github/7".into(),
                    folder: None,
                    paths: Some(vec!["users/get.yml".into()]),
                }),
            ),
            (
                "revisionRun",
                "RevisionRun",
                j(RevisionRun {
                    commit: commit("b", "Users v2"),
                    steps: vec![run_step()],
                    summary: summary(),
                }),
            ),
            (
                "compareRequest",
                "CompareRequest",
                j(CompareRequest {
                    from: "main".into(),
                    to: "users-v2".into(),
                    folder: Some("users".into()),
                    paths: None,
                }),
            ),
            (
                "runComparison",
                "RunComparison",
                j(RunComparison {
                    from: commit("a", "main"),
                    to: commit("b", "Users v2"),
                    requests: vec![RequestComparison {
                        path: "users/get.yml".into(),
                        name: "Get user".into(),
                        same: false,
                        status: Some(ValueChange {
                            old: Some(200),
                            new: Some(404),
                        }),
                        headers: vec![HeaderChange {
                            name: "content-type".into(),
                            old: Some("application/json".into()),
                            new: None,
                        }],
                        body: vec![BodyChange {
                            pointer: "/items/0/id".into(),
                            old: Some(serde_json::json!(1)),
                            new: None,
                        }],
                        body_text: false,
                        tests: vec![TestChange {
                            name: "status is 200".into(),
                            old: Some(true),
                            new: Some(false),
                        }],
                        error: Some(ValueChange {
                            old: None,
                            new: Some("connection refused".into()),
                        }),
                        sides: Some(crate::response_sides()),
                    }],
                    from_summary: summary(),
                    to_summary: summary(),
                }),
            ),
            (
                "bisectRequest",
                "BisectRequest",
                j(BisectRequest {
                    good: "v1.0".into(),
                    bad: "HEAD".into(),
                    folder: None,
                }),
            ),
            (
                "bisectOutcome",
                "BisectOutcome",
                j(BisectOutcome {
                    first_bad: Some(commit("c", "Break it")),
                    candidates: 49,
                    steps: [BisectVerdict::Good, BisectVerdict::Bad, BisectVerdict::Skip]
                        .into_iter()
                        .map(|verdict| BisectStep {
                            commit: commit("c", "Break it"),
                            verdict,
                            failed: vec!["Get user: status is 200".into()],
                            remaining: 12,
                        })
                        .collect(),
                }),
            ),
            ("testRecord", "TestRecord", j(test_record())),
            (
                "requestTimeline",
                "RequestTimeline",
                j(RequestTimeline {
                    path: "users/get.yml".into(),
                    entries: vec![TimelineEntry {
                        commit: GitFileCommit {
                            commit: commit("b", "v2"),
                            path: "users/get.yml".into(),
                            change: GitFileChange::Modified,
                            from: None,
                        },
                        fields: vec![field_change()],
                        tests: vec![test_record()],
                    }],
                }),
            ),
            (
                "fieldBlames",
                "FieldBlame[]",
                j(vec![FieldBlame {
                    field: path(&["url"]),
                    commit: commit("b", "v2"),
                    value: text("{{baseUrl}}/v2/users"),
                }]),
            ),
            (
                "reviewHosts",
                "ReviewHost[]",
                j(vec![ReviewHost {
                    host: "git.example.com:8443".into(),
                    kind: ReviewKind::Github,
                    api: "https://git.example.com:8443/api/v3".into(),
                }]),
            ),
            (
                "reviewRepo",
                "ReviewRepo",
                j(ReviewRepo {
                    kind: ReviewKind::Gitlab,
                    host: "gitlab.example.com".into(),
                    api: "https://gitlab.example.com/api/v4".into(),
                    project: "group/sub/api".into(),
                }),
            ),
            (
                "pullRequests",
                "PullRequest[]",
                j([PullState::Open, PullState::Closed, PullState::Merged]
                    .into_iter()
                    .map(|state| PullRequest { state, ..pull() })
                    .collect::<Vec<_>>()),
            ),
            (
                "reviewComments",
                "ReviewComment[]",
                j(vec![
                    ReviewComment {
                        id: "1".into(),
                        author: "grace".into(),
                        body: "Why v2?".into(),
                        created: 1_795_000_000,
                        path: Some("users/get.yml".into()),
                        line: Some(4),
                        field: Some(path(&["url"])),
                        outdated: false,
                    },
                    ReviewComment {
                        id: "2".into(),
                        author: "ada".into(),
                        body: "LGTM".into(),
                        created: 1_795_000_100,
                        path: None,
                        line: None,
                        field: None,
                        outdated: false,
                    },
                ]),
            ),
            (
                "newReviewComment",
                "NewReviewComment",
                j(NewReviewComment {
                    path: "users/get.yml".into(),
                    line: 4,
                    body: "Why v2?".into(),
                }),
            ),
            (
                "reviewSubmissions",
                "ReviewSubmission[]",
                j([
                    ReviewVerdict::Approve,
                    ReviewVerdict::RequestChanges,
                    ReviewVerdict::Comment,
                ]
                .into_iter()
                .map(|verdict| ReviewSubmission {
                    verdict,
                    body: "".into(),
                })
                .collect::<Vec<_>>()),
            ),
            (
                "ciStatus",
                "CiStatus",
                j(CiStatus {
                    state: CiState::Failure,
                    checks: [
                        CiState::Pending,
                        CiState::Success,
                        CiState::Failure,
                        CiState::Neutral,
                        CiState::None,
                    ]
                    .into_iter()
                    .map(|state| CiCheck {
                        name: "rt run".into(),
                        state,
                        url: None,
                    })
                    .collect(),
                }),
            ),
            (
                "reviewSession",
                "ReviewSession",
                j(ReviewSession {
                    pull: pull(),
                    head_ref: rt_app::review::review_ref(ReviewKind::Github, 7),
                    merge_base: "a".repeat(40),
                }),
            ),
            (
                "reviewErrors",
                "ReviewError[]",
                j(vec![
                    ReviewError::NoToken("github.com".into()),
                    ReviewError::Auth("token expired".into()),
                    ReviewError::NotFound("pull 9".into()),
                    ReviewError::Transport("timeout".into()),
                    ReviewError::Host("422".into()),
                ]),
            ),
            (
                "contractSpec",
                "ContractSpec",
                j(ContractSpec {
                    path: "/openapi.yaml".into(),
                    title: "Orders API".into(),
                    version: "1.3.0".into(),
                    configured: false,
                }),
            ),
            (
                "contractReport",
                "ContractReport",
                j(ContractReport {
                    findings: [
                        FindingKind::NoOperation,
                        FindingKind::UnknownParameter,
                        FindingKind::MissingParameter,
                        FindingKind::BodyMismatch,
                    ]
                    .into_iter()
                    .map(|kind| ContractFinding {
                        request: "users/get.yml".into(),
                        kind,
                        operation: Some("GET /users/{id}".into()),
                        message: "x".into(),
                    })
                    .collect(),
                    uncovered: vec!["DELETE /users/{id}".into()],
                    operations: 5,
                    covered: 4,
                }),
            ),
            (
                "specChanges",
                "SpecChanges",
                j(SpecChanges {
                    from_version: Some("1.2.0".into()),
                    to_version: None,
                    changes: [
                        SpecChangeKind::OperationAdded,
                        SpecChangeKind::OperationRemoved,
                        SpecChangeKind::ParameterAdded,
                        SpecChangeKind::ParameterRemoved,
                        SpecChangeKind::RequiredAdded,
                        SpecChangeKind::FieldRemoved,
                        SpecChangeKind::FieldAdded,
                        SpecChangeKind::TypeChanged,
                        SpecChangeKind::ResponseChanged,
                        SpecChangeKind::Other,
                    ]
                    .into_iter()
                    .map(|kind| SpecChange {
                        kind,
                        breaking: true,
                        operation: None,
                        location: "response 200: items[].id".into(),
                        message: "x".into(),
                        requests: vec!["users/get.yml".into()],
                    })
                    .collect(),
                }),
            ),
            (
                "historyScan",
                "HistoryScan",
                j(HistoryScan {
                    commits: 50,
                    findings: vec![HistoryFinding {
                        id: format!("{}:users/get.yml:4", "a".repeat(40)),
                        finding: SecretFinding {
                            path: "users/get.yml".into(),
                            line: 4,
                            kind: "bearer token".into(),
                            snippet: "Authorization: Bearer ***".into(),
                            hint: "Use a {{variable}}".into(),
                        },
                        first: commit("a", "Add"),
                        commits: vec!["a".repeat(40)],
                        branches: vec!["main".into()],
                        in_head: true,
                    }],
                }),
            ),
            (
                "scanProgress",
                "ScanProgress",
                j(ScanProgress {
                    scanned: 10,
                    total: 50,
                }),
            ),
            (
                "rescuePlan",
                "RescuePlan",
                j(RescuePlan {
                    findings: vec![format!("{}:users/get.yml:4", "a".repeat(40))],
                    variable: "apiToken".into(),
                    environment: "Local".into(),
                }),
            ),
            (
                "rescueResult",
                "RescueResult",
                j(RescueResult {
                    rewritten: 12,
                    branches: vec![RewrittenBranch {
                        name: "main".into(),
                        old: "a".repeat(40),
                        new: "b".repeat(40),
                        pushed: true,
                        remote: Some("origin".into()),
                    }],
                    backup: format!("{}1795000000/", rt_app::rescue::BACKUP_PREFIX),
                    tags: vec!["v1.0".into()],
                }),
            ),
            ("wave12RequestSpec", "RequestSpec", j(request_spec())),
        ]
    }
}

/// Wave 17 interface: snapshot tests and sharing without a server.
mod wave17 {
    use std::collections::BTreeMap;

    use rt_app::share::{
        CopiedAs, CopyKind, CopyOptions, SHARE_VERSION, ShareLink, ShareOptions, SharePayload,
        ShareVariable,
    };
    use rt_app::snapshots::{
        FreezeOptions, SNAPSHOT_VERSION, Snapshot, SnapshotBody, SnapshotCheck, SnapshotDiff,
        SnapshotDiffKind, SnapshotEntry, SnapshotFreeze, SnapshotState,
    };
    use serde::Serialize;
    use serde_json::json;

    fn j<T: Serialize>(v: T) -> serde_json::Value {
        serde_json::to_value(v).unwrap()
    }

    fn snapshot(body: SnapshotBody) -> Snapshot {
        Snapshot {
            version: SNAPSHOT_VERSION,
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
            body,
            ignore: vec!["$.items[*].createdAt".into()],
        }
    }

    fn check(state: SnapshotState, differences: Vec<SnapshotDiff>) -> SnapshotCheck {
        SnapshotCheck {
            request_path: "orders/get.yml".into(),
            snapshot: "orders/get.snap.json".into(),
            state,
            differences,
        }
    }

    pub fn fixtures() -> Vec<(&'static str, &'static str, serde_json::Value)> {
        let diff = |path: &str, kind, expected, actual| SnapshotDiff {
            path: path.into(),
            kind,
            expected,
            actual,
        };
        vec![
            (
                "snapshot",
                "Snapshot",
                j(snapshot(SnapshotBody::Value(
                    json!({ "items": [{ "id": 1 }] }),
                ))),
            ),
            (
                "snapshotBodies",
                "Snapshot[]",
                j(vec![
                    snapshot(SnapshotBody::Value(json!("plain text"))),
                    snapshot(SnapshotBody::Value(json!(null))),
                    snapshot(SnapshotBody::Digest {
                        digest: "9f86d081884c7d659a2feaa0c55ad015".into(),
                        length: 4_194_304,
                    }),
                ]),
            ),
            (
                "snapshotChecks",
                "SnapshotCheck[]",
                j(vec![
                    check(SnapshotState::Matches, Vec::new()),
                    check(
                        SnapshotState::Differs,
                        vec![
                            diff(
                                "status",
                                SnapshotDiffKind::Changed,
                                Some(json!(200)),
                                Some(json!(201)),
                            ),
                            diff(
                                "body.name",
                                SnapshotDiffKind::Added,
                                None,
                                Some(json!("Ada")),
                            ),
                            diff("body.age", SnapshotDiffKind::Removed, Some(json!(36)), None),
                            diff(
                                "body.id",
                                SnapshotDiffKind::Type,
                                Some(json!(1)),
                                Some(json!("1")),
                            ),
                        ],
                    ),
                    check(SnapshotState::Missing, Vec::new()),
                    check(SnapshotState::None, Vec::new()),
                ]),
            ),
            (
                "snapshotFreeze",
                "SnapshotFreeze",
                j(SnapshotFreeze {
                    request_path: "orders/get.yml".into(),
                    snapshot: "orders/get.snap.json".into(),
                    assertion_added: true,
                    content: snapshot(SnapshotBody::Value(json!({ "ok": true }))),
                }),
            ),
            (
                "freezeOptions",
                "FreezeOptions[]",
                j(vec![
                    FreezeOptions::default(),
                    FreezeOptions {
                        keep_etag: true,
                        file: Some("orders.snap.json".into()),
                    },
                ]),
            ),
            (
                "snapshotEntries",
                "SnapshotEntry[]",
                j(vec![
                    SnapshotEntry {
                        request_path: "orders/get.yml".into(),
                        snapshot: "orders/get.snap.json".into(),
                        exists: true,
                    },
                    SnapshotEntry {
                        request_path: "orders/list.yml".into(),
                        snapshot: "orders/list.snap.json".into(),
                        exists: false,
                    },
                ]),
            ),
            (
                "sharePayload",
                "SharePayload",
                j(SharePayload {
                    version: SHARE_VERSION,
                    name: "Get order".into(),
                    request: super::request_spec(),
                    variables: vec![
                        ShareVariable {
                            name: "baseUrl".into(),
                            value: Some("https://api.example.com".into()),
                            secret: false,
                        },
                        ShareVariable {
                            name: "token".into(),
                            value: None,
                            secret: true,
                        },
                    ],
                }),
            ),
            (
                "shareOptions",
                "ShareOptions[]",
                j(vec![
                    ShareOptions::default(),
                    ShareOptions {
                        include_values: true,
                        environment: Some("Local".into()),
                        base: Some("https://app.example.com/".into()),
                    },
                ]),
            ),
            (
                "shareLink",
                "ShareLink",
                j(ShareLink {
                    text: "rt:1.eJxLzs8tSM0rSS0q".into(),
                    preview: "{\n  \"version\": 1\n}".into(),
                    payload: SharePayload {
                        version: SHARE_VERSION,
                        name: "Get order".into(),
                        request: super::request_spec(),
                        variables: Vec::new(),
                    },
                    length: 21,
                    fits_qr: true,
                }),
            ),
            (
                "copyKinds",
                "CopyKind[]",
                j([CopyKind::Curl, CopyKind::Fetch, CopyKind::Python]),
            ),
            (
                "copyOptions",
                "CopyOptions[]",
                j(vec![
                    CopyOptions::default(),
                    CopyOptions {
                        resolve_variables: true,
                        environment: Some("Local".into()),
                    },
                ]),
            ),
            (
                "copiedAs",
                "CopiedAs",
                j(CopiedAs {
                    kind: CopyKind::Curl,
                    text: "curl -H \"Authorization: Bearer $TOKEN\" \"$BASE_URL/orders/1\"".into(),
                    notes: vec!["GraphQL variables are left out".into()],
                }),
            ),
        ]
    }
}

fn render() -> String {
    let fixtures = fixtures();
    let mut types: Vec<&str> = fixtures
        .iter()
        .map(|(_, t, _)| t.trim_end_matches("[]"))
        .collect();
    types.sort_unstable();
    types.dedup();
    let mut out = String::from(
        "// SPDX-License-Identifier: Apache-2.0\n\
         // Copyright 2026 The RequestTailor Authors\n\n\
         // GENERATED by `RT_UPDATE_CONTRACT=1 cargo test -p rt-app --test contract`. Do not edit.\n\
         // Real serde JSON from the Rust types, annotated with the TypeScript types in ./types.ts:\n\
         // `npm run check` fails if they drift apart.\n\n",
    );
    out.push_str(&format!(
        "import type {{ {} }} from \"./types\";\n",
        types.join(", ")
    ));
    for (name, ty, value) in fixtures {
        out.push_str(&format!(
            "\nexport const {name}: {ty} = {};\n",
            serde_json::to_string_pretty(&value).unwrap()
        ));
    }
    out
}

#[test]
fn typescript_contract_is_up_to_date() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/ui/src/commands/contract.generated.ts"
    );
    let expected = render();
    if std::env::var_os("RT_UPDATE_CONTRACT").is_some() {
        std::fs::write(path, &expected).unwrap();
        return;
    }
    let actual = std::fs::read_to_string(path).unwrap_or_default();
    assert!(
        actual == expected,
        "{path} is stale; run `RT_UPDATE_CONTRACT=1 cargo test -p rt-app --test contract`"
    );
}

/// What the UI sends back must deserialize to the same value (OAuth 2.0 flattens its flow
/// into the tagged `Auth`, which serde only round-trips with buffering).
#[test]
fn wave5_values_round_trip_through_json() {
    for auth in auths() {
        let json = serde_json::to_string(&auth).unwrap();
        assert_eq!(serde_json::from_str::<Auth>(&json).unwrap(), auth, "{json}");
    }
    let env = environment_with_config();
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(serde_json::from_str::<Environment>(&json).unwrap(), env);
    let reply = view_reply();
    let json = serde_json::to_string(&reply).unwrap();
    assert_eq!(serde_json::from_str::<ViewReply>(&json).unwrap(), reply);
    let report = report_ok();
    let json = serde_json::to_string(&report).unwrap();
    assert_eq!(serde_json::from_str::<SendReport>(&json).unwrap(), report);
}

/// The OpenAPI import wizard and the run wizard.
mod openapi_wizard {
    use rt_app::contract::{
        DifferencePlace, InputField, InputPlace, InputProblem, ProblemKind, ResponseCheck,
        ResponseDifference, SendRequirements, ServerOption,
    };
    use rt_app::openapi::{
        FetchedFile, FetchedSpec, OpenApiExamples, OpenApiGrouping, OpenApiNaming, OpenApiOptions,
        OpenApiServerChoice, SpecOperation, SpecOverview, SpecSecurityScheme, SpecServer,
        SpecServerVariable, SpecTag,
    };
    use rt_format::import::{Loss, LossEffect, LossKind};
    use serde::Serialize;

    fn j<T: Serialize>(v: T) -> serde_json::Value {
        serde_json::to_value(v).unwrap()
    }

    fn loss() -> Loss {
        Loss {
            location: "collection".into(),
            kind: LossKind::Other,
            effect: LossEffect::Dropped,
            detail: "references to other files not resolved (`common.yaml`)".into(),
        }
    }

    pub fn fixtures() -> Vec<(&'static str, &'static str, serde_json::Value)> {
        let op = |grouping, naming, examples| OpenApiOptions {
            operations: Some(vec!["GET /pets".into()]),
            grouping,
            naming,
            examples,
            servers: Some(vec![OpenApiServerChoice {
                index: 0,
                environment: Some("Production".into()),
            }]),
            default_server: Some(0),
            collection_name: Some("Petstore".into()),
            keep_spec: true,
        };
        let field = |place, kind: Option<ProblemKind>| InputField {
            id: "path:petId".into(),
            place,
            name: "petId".into(),
            required: true,
            secret: false,
            auth: false,
            value: Some(String::new()),
            problem: kind.map(|kind| InputProblem {
                kind,
                message: "petId is required".into(),
                details: vec!["$.name: required property `name` is missing".into()],
            }),
            schema: Some(serde_json::json!({ "type": "string", "format": "uuid" })),
            description: Some("The id of the pet".into()),
            example: Some("42".into()),
        };
        vec![
            (
                "specOverview",
                "SpecOverview",
                j(SpecOverview {
                    title: "Petstore".into(),
                    version: "1.0.0".into(),
                    description: Some("A sample API".into()),
                    spec_version: "3.0.3".into(),
                    main_file: "petstore.yaml".into(),
                    files: 2,
                    servers: vec![SpecServer {
                        url: "https://{region}.example.com/v1".into(),
                        description: Some("Production".into()),
                        suggested_name: "Production".into(),
                        variables: vec![SpecServerVariable {
                            name: "region".into(),
                            default: "eu".into(),
                            values: vec!["eu".into(), "us".into()],
                            description: None,
                        }],
                    }],
                    tags: vec![SpecTag {
                        name: "pets".into(),
                        description: None,
                        operations: 3,
                    }],
                    operations: vec![SpecOperation {
                        key: "GET /pets/{petId}".into(),
                        method: "GET".into(),
                        path: "/pets/{petId}".into(),
                        tags: vec!["pets".into()],
                        summary: Some("Info for a specific pet".into()),
                        operation_id: Some("showPetById".into()),
                        deprecated: false,
                    }],
                    security: vec![SpecSecurityScheme {
                        name: "bearerAuth".into(),
                        kind: "bearer".into(),
                        detail: "JWT".into(),
                        maps_to: Some("Bearer token {{token}}".into()),
                        variables: vec!["token".into()],
                    }],
                    losses: vec![loss()],
                }),
            ),
            (
                "openApiOptions",
                "OpenApiOptions",
                j(op(
                    OpenApiGrouping::Tag,
                    OpenApiNaming::Summary,
                    OpenApiExamples::Spec,
                )),
            ),
            (
                "openApiOptionsVariants",
                "OpenApiOptions[]",
                j(vec![
                    op(
                        OpenApiGrouping::Path,
                        OpenApiNaming::OperationId,
                        OpenApiExamples::Schema,
                    ),
                    op(
                        OpenApiGrouping::Flat,
                        OpenApiNaming::Summary,
                        OpenApiExamples::Spec,
                    ),
                    OpenApiOptions::default(),
                ]),
            ),
            (
                "fetchedSpec",
                "FetchedSpec",
                j(FetchedSpec {
                    url: "https://api.example.com/openapi.yaml".into(),
                    files: vec![FetchedFile {
                        name: "openapi.yaml".into(),
                        text: "openapi: 3.0.3".into(),
                    }],
                    losses: vec![loss()],
                }),
            ),
            (
                "sendRequirements",
                "SendRequirements",
                j(SendRequirements {
                    operation: "GET /pets/{petId}".into(),
                    summary: Some("Info for a specific pet".into()),
                    deprecated: false,
                    spec: "openapi.yaml".into(),
                    ready: false,
                    fields: vec![
                        field(InputPlace::Path, Some(ProblemKind::Missing)),
                        field(InputPlace::Variable, Some(ProblemKind::Invalid)),
                        field(InputPlace::Query, None),
                        field(InputPlace::Header, None),
                        InputField {
                            schema: None,
                            description: None,
                            example: None,
                            value: None,
                            problem: Some(InputProblem {
                                kind: ProblemKind::Missing,
                                message: "a body is required".into(),
                                details: Vec::new(),
                            }),
                            ..field(InputPlace::Body, None)
                        },
                    ],
                    servers: vec![ServerOption {
                        url: "https://{{region}}.example.com/v1".into(),
                        description: None,
                    }],
                }),
            ),
            (
                "responseCheck",
                "ResponseCheck",
                j(ResponseCheck {
                    operation: "GET /pets/{petId}".into(),
                    status: 200,
                    matched: Some("200".into()),
                    differences: [
                        DifferencePlace::Status,
                        DifferencePlace::Header,
                        DifferencePlace::ContentType,
                        DifferencePlace::Body,
                    ]
                    .into_iter()
                    .map(|place| ResponseDifference {
                        place,
                        path: "$.id".into(),
                        expected: "integer".into(),
                        got: "string".into(),
                    })
                    .collect(),
                    body_skipped: Some("the body is larger than 8 MB".into()),
                }),
            ),
        ]
    }
}

/// Wave 18 (interface): checking and watching. The values are written as JSON and read back
/// through the Rust types, so a field the types lack (or lack a default for) fails here.
mod wave18 {
    use serde::Serialize;
    use serde::de::DeserializeOwned;
    use serde_json::json;

    fn via<T: Serialize + DeserializeOwned>(v: serde_json::Value) -> serde_json::Value {
        let typed: T = serde_json::from_value(v).expect("the fixture reads as its Rust type");
        serde_json::to_value(typed).unwrap()
    }

    pub fn fixtures() -> Vec<(&'static str, &'static str, serde_json::Value)> {
        use rt_app::catalogue::{CatalogueUpdate, InstallPlan, InstalledSource, SourceIndex};
        use rt_app::envcompare::{EnvCompareRequest, EnvComparison};
        use rt_app::inbox::{CapturedRequest, InboxPage, InboxSettings, InboxStatus};
        use rt_app::inspect::{FoundToken, InspectTarget, JwtInfo, Observation};
        use rt_app::lint::{LintReport, LintSettings};
        use rt_app::paging::{EffectivePaging, PagingRun, PagingSpec, PagingStop};
        use rt_app::views::{ResponseView, ViewResult};
        use rt_core::TimingSegment;
        use rt_core::ratelimit::{RateLimits, Throttle};
        use rt_core::runner::{RunEvent, RunPlan};
        use rt_core::send::SendReport;

        let jwt = json!({
            "header": { "alg": "RS256", "typ": "JWT" },
            "algorithm": "RS256",
            "claims": [
                { "name": "sub", "value": "user-1", "kind": "text", "meaning": "subject" },
                { "name": "exp", "value": 1_900_000_000, "kind": "time", "meaning": null },
                { "name": "scope", "value": ["a", "b"], "kind": "list", "meaning": null },
                { "name": "x", "value": { "k": 1 }, "kind": "other", "meaning": null }
            ],
            "warnings": [
                { "kind": "expired", "text": "expired 2 days ago" },
                { "kind": "not-yet-valid", "text": "valid in 1 hour" },
                { "kind": "expires-soon", "text": "expires in 5 minutes" },
                { "kind": "algorithm-none", "text": "unsigned token" }
            ],
            "expires_at": 1_900_000_000,
            "signature_verified": false
        });
        let plan = json!({
            "source": "https://example.com/plugins.json",
            "entry": {
                "id": "acme", "name": "Acme", "version": "1.2.0", "api": 1,
                "description": "Acme helpers", "license": "Apache-2.0", "homepage": null,
                "wasm": { "url": "https://example.com/acme.wasm", "sha256": "ab".repeat(32), "size": 2048 },
                "permissions": { "network": ["api.example.com"], "storage": true, "variables": false }
            },
            "sha256": "ab".repeat(32),
            "permissions": { "network": ["api.example.com"], "storage": true, "variables": false },
            "permissions_differ": false,
            "update": false,
            "more_permissions": false,
            "warnings": []
        });
        let view = json!({ "name": "ids", "language": "jsonpath", "expression": "$.items[*].id", "display": "table", "columns": ["id"] });
        vec![
            ("timingSegments", "TimingSegment[]", via::<Vec<TimingSegment>>(json!([
                { "id": "db", "label": "Database", "ms": 41, "parent": "server" },
                { "id": "queue", "label": "Queue", "ms": 3 }
            ]))),
            ("rateLimits", "RateLimits", via::<RateLimits>(json!({
                "limit": 100, "remaining": 7, "reset_ms": 30000, "retry_after_ms": 1000,
                "windows": [{ "limit": 100, "seconds": 60 }]
            }))),
            ("throttle", "Throttle", via::<Throttle>(json!({ "mode": "respect", "max_wait_ms": 60000, "retries": 2 }))),
            ("runPlanThrottled", "RunPlan", via::<RunPlan>(json!({
                "requests": ["a.yml"], "data": [], "iterations": 1, "delay_ms": 0, "bail": false,
                "throttle": { "mode": "respect", "max_wait_ms": 1000, "retries": 1 }
            }))),
            ("runEventWaiting", "RunEvent", via::<RunEvent>(json!({
                "kind": "waiting", "request_path": "a.yml", "until": 1_900_000_000_000_i64, "reason": "rate limit"
            }))),
            ("sendReportWatched", "SendReport", via::<SendReport>(json!({
                "version": 1, "request_path": "a.yml", "environment": null, "request": null,
                "response": null, "variables": [], "unresolved": [], "error": null,
                "segments": [{ "id": "db", "label": "Database", "ms": 41, "parent": "server" }],
                "rate_limit": { "limit": 100, "remaining": 7 },
                "checks": [{ "id": "cookie/no-secure", "severity": "warn", "location": "set-cookie: sid", "why": "sent over HTTP", "fix": "add Secure" }]
            }))),
            ("testRecordTimed", "TestRecord", via::<rt_app::timeline::TestRecord>(json!({
                "collection": "shop", "request": "orders/get.yml", "environment": null, "time": 1_900_000_000,
                "source": "runner", "passed": true, "status": 200, "tests": [], "duration_ms": 87
            }))),
            ("pagingRules", "PagingSpec[]", via::<Vec<PagingSpec>>(json!([
                { "rule": { "kind": "link-header" } },
                { "rule": { "kind": "json-url", "next": "$.links.next" }, "merge": "$.items" },
                { "rule": { "kind": "cursor", "next": "$.next", "param": { "place": "query", "name": "cursor" } }, "merge": "$.data" },
                { "rule": { "kind": "cursor", "next": "$.next", "param": { "place": "body", "name": "after" } } },
                { "rule": { "kind": "cursor", "next": "$.next", "param": { "place": "header", "name": "x-cursor" } } }
            ]))),
            ("effectivePagings", "EffectivePaging[]", via::<Vec<EffectivePaging>>(json!([
                { "spec": { "rule": { "kind": "link-header" } }, "origin": { "kind": "request" } },
                { "spec": { "rule": { "kind": "link-header" } }, "origin": { "kind": "collection" } },
                { "spec": { "rule": { "kind": "link-header" } }, "origin": { "kind": "plugin", "plugin": "odata", "id": "odata-next" } }
            ]))),
            ("pagingRun", "PagingRun", via::<PagingRun>(json!({
                "path": "a.yml", "input": { "environment": null, "vars": [] },
                "mode": "all", "response_id": 1,
                "limits": { "max_pages": 20, "max_bytes": 52_428_800, "max_seconds": 120 }
            }))),
            ("pagingStops", "PagingStop[]", via::<Vec<PagingStop>>(json!([
                { "kind": "done" }, { "kind": "max-pages" }, { "kind": "max-bytes" },
                { "kind": "max-seconds" }, { "kind": "loop" },
                { "kind": "other-origin", "message": "https://other.example/next" },
                { "kind": "error-status", "message": 500 },
                { "kind": "refused", "message": "blocked by policy" },
                { "kind": "not-read" }
            ]))),
            ("pagingContribution", "PagingContribution", via::<rt_plugin::api::PagingContribution>(json!({
                "id": "odata-next",
                "match": { "hosts": ["*.example.com"], "content_types": ["application/json"], "url_pattern": "/odata/" },
                "rule": { "rule": { "kind": "json-url", "next": "$['@odata.nextLink']" }, "merge": "$.value" }
            }))),
            ("envCompareRequest", "EnvCompareRequest", via::<EnvCompareRequest>(json!({
                "environments": ["Dev", "Test", "Prod"], "folder": "orders", "paths": null,
                "ignore_env_values": true, "production_confirmed": false
            }))),
            ("envComparison", "EnvComparison", via::<EnvComparison>(json!({
                "reference": "Dev", "pairs": [],
                "skipped": [{ "path": "orders/create.yml", "name": "Create order", "reason": "writes (POST)" }]
            }))),
            ("jwtInfo", "JwtInfo", via::<JwtInfo>(jwt.clone())),
            ("foundTokens", "FoundToken[]", via::<Vec<FoundToken>>(json!([
                { "location": { "kind": "authorization" }, "token": jwt },
                { "location": { "kind": "auth" }, "token": jwt },
                { "location": { "kind": "query" }, "token": jwt },
                { "location": { "kind": "body", "path": "$.access_token" }, "token": jwt },
                { "location": { "kind": "cookie", "name": "sid" }, "token": jwt }
            ]))),
            ("observations", "Observation[]", via::<Vec<Observation>>(json!([
                { "id": "cookie/no-secure", "severity": "warn", "location": "set-cookie: sid", "why": "w", "fix": "f" },
                { "id": "header/server-version", "severity": "info", "location": "server", "why": "w", "fix": "f" }
            ]))),
            ("inspectTarget", "InspectTarget", via::<InspectTarget>(json!({ "path": "a.yml", "environment": "Dev", "response_id": 3 }))),
            ("inboxSettings", "InboxSettings", via::<InboxSettings>(json!({
                "port": 47855, "status": 202, "body": "{\"ok\":true}", "content_type": "application/json", "delay_ms": 250
            }))),
            ("inboxStatus", "InboxStatus", via::<InboxStatus>(json!({
                "running": true, "port": 47855, "url": "http://127.0.0.1:47855/", "held": 3, "dropped": 0
            }))),
            ("capturedRequest", "CapturedRequest", via::<CapturedRequest>(json!({
                "id": 1, "at": 1_900_000_000_000_i64, "method": "POST", "target": "/hook?x=1",
                "headers": [["content-type", "application/json"]], "body": "{}", "size": 2
            }))),
            ("inboxPage", "InboxPage", via::<InboxPage>(json!({
                "events": [
                    { "kind": "started", "port": 47855 },
                    { "kind": "request", "request": { "id": 1, "at": 1, "method": "GET", "target": "/", "headers": [], "body": null, "size": 0 } },
                    { "kind": "dropped", "count": 5 },
                    { "kind": "cleared" },
                    { "kind": "stopped" }
                ],
                "next": 5
            }))),
            ("lintSettings", "LintSettings", via::<LintSettings>(json!({
                "rules": { "lint/no-tests": "warn", "lint/duplicate-name": "error" },
                "ignore": ["orders/legacy.yml"], "min_score": 80
            }))),
            ("lintReport", "LintReport", via::<LintReport>(json!({
                "collection": "shop",
                "score": { "score": 72, "rules": [{ "rule": "no-tests", "ok": 8, "total": 10 }] },
                "folders": [{ "folder": "orders", "score": { "score": 60, "rules": [] } }],
                "findings": [
                    { "rule": "no-tests", "level": "warn", "path": "orders/get.yml", "message": "no tests", "location": null },
                    { "rule": "hard-coded-host", "level": "error", "path": "a.yml", "message": "host", "location": "url" },
                    { "rule": "orphan-variable", "level": "info", "path": "", "message": "unused", "location": null },
                    { "rule": "duplicate-name", "level": "off", "path": "b.yml", "message": "dup", "location": null }
                ],
                "ignored": 1
            }))),
            ("responseViews", "ResponseView[]", via::<Vec<ResponseView>>(json!([
                view.clone(),
                { "name": "all", "language": "jq", "expression": ".", "display": "json" },
                { "name": "text", "language": "jsonpath", "expression": "$.message", "display": "text" }
            ]))),
            ("viewResult", "ViewResult", via::<ViewResult>(json!({
                "display": "table", "values": [1, 2], "columns": ["id"], "rows": [[1], [2]], "notice": "2 of 2 shown"
            }))),
            ("sourceIndex", "SourceIndex", via::<SourceIndex>(json!({
                "source": { "url": "https://example.com/plugins.json", "commit": "abc123", "name": "Example" },
                "index": { "version": 1, "name": "Example", "plugins": [plan["entry"].clone()] }
            }))),
            ("installPlan", "InstallPlan", via::<InstallPlan>(plan)),
            ("installedSource", "InstalledSource", via::<InstalledSource>(json!({
                "plugin": "acme", "source": "https://example.com/plugins.json", "version": "1.2.0", "sha256": "ab".repeat(32)
            }))),
            ("catalogueUpdate", "CatalogueUpdate", via::<CatalogueUpdate>(json!({
                "plugin": "acme", "source": "https://example.com/plugins.json", "installed": "1.1.0", "available": "1.2.0"
            }))),
            ("specChangeKinds", "SpecChange[]", json!([
                "type-removed", "enum-value-removed", "nullability-changed", "number-changed", "streaming-changed"
            ].iter().map(|k| json!({
                "kind": k, "breaking": true, "operation": null, "location": "Query.user", "message": "m", "requests": []
            })).collect::<Vec<_>>())),
        ]
    }
}

mod wave19 {
    use serde::Serialize;
    use serde::de::DeserializeOwned;
    use serde_json::json;

    fn via<T: Serialize + DeserializeOwned>(v: serde_json::Value) -> serde_json::Value {
        let typed: T = serde_json::from_value(v).expect("the fixture reads as its Rust type");
        serde_json::to_value(typed).unwrap()
    }

    pub fn fixtures() -> Vec<(&'static str, &'static str, serde_json::Value)> {
        use rt_app::access::{AccessCheck, AccessOptions, AccessProfile};
        use rt_app::consumers::{ConsumerRef, ConsumerReport, ConsumerVerifyRequest};
        use rt_app::egress::{EgressError, EgressHost, EgressRequest, EgressResponse};
        use rt_app::journal::{
            CompensationItem, EntryStates, JournalPage, JournalQuery, JournalSettings,
            ManualRevert, PluginCompensationRule, RevertOptions, RevertOutcome, RevertPlan,
        };
        use rt_app::notebook::{Notebook, NotebookEntry, NotebookOutcome, NotebookRun};
        use rt_app::security::{SecurityFinding, SecurityRule, SecuritySettings, SecurityTarget};
        use rt_app::sequence::{SequenceModel, SequenceOptions, SequenceSource, SequenceText};
        use rt_app::shrink::{ShrinkRequest, ShrinkResult};
        use rt_core::TimingSegment;
        use rt_core::model::RequestSpec;
        use rt_core::trace::TraceRef;
        use rt_core::transport::TlsInfo;
        use rt_script::debug::{DebugCommand, DebugState, DebugTarget};

        let spec = serde_json::to_value(RequestSpec::default()).unwrap();
        let compensation = json!({
            "kind": "delete", "request": spec.clone(), "state_hash": "ab12", "probe": spec.clone(),
            "description": "Delete the order again"
        });
        let plan = json!({
            "environment": "Test", "production": false, "truncated": false,
            "steps": [
                { "entry_id": 1, "state": "planned", "method": "POST", "url": "https://x/orders",
                  "effect": "write", "description": "Delete order 7", "compensation": compensation.clone() },
                { "entry_id": 2, "state": "not-reversible", "method": "POST", "url": "https://x/mail",
                  "effect": "write", "description": "Mail sent", "reason": "a sent mail cannot be undone" }
            ]
        });
        let check = json!({
            "path": "orders/get.yml",
            "verdict": { "kind": "broken-object-access" },
            "findings": [{
                "verdict": { "kind": "broken-object-access" }, "message": "B read A's order",
                "evidence": [{ "profile": "B", "status": 200, "header_names": ["content-type"], "values": [["id", "7"]] }]
            }],
            "status_a": 200, "status_b": 200
        });
        let shrink = json!({
            "request": spec.clone(), "removed": [{ "part": "header:x-a", "kind": "header" }],
            "probes": 12, "not_reproduced": false, "flaky": false, "one_minimal": true, "stopped_by": null
        });
        let model = json!({
            "participants": [
                { "id": "client", "label": "Client", "kind": "client" },
                { "id": "api", "label": "api.example.com", "kind": "server" }
            ],
            "messages": [
                { "kind": "request", "from": "client", "to": "api", "label": "GET /orders", "request_path": "orders/get.yml" },
                { "kind": "response", "from": "api", "to": "client", "label": "200 12 ms" }
            ],
            "notes": [{ "after": 0, "anchor": { "kind": "over", "participant": "api" }, "text": "cached" },
                      { "after": 1, "anchor": { "kind": "between", "from": "client", "to": "api" }, "text": "n" }],
            "groups": [{ "kind": "loop", "label": "pages", "from": 0, "to": 1 }],
            "omitted": 0
        });
        let debug_state = json!({
            "session": 1, "status": "paused",
            "pause": {
                "line": 3, "reason": "breakpoint",
                "frames": [{ "function": "main", "line": 3 }],
                "scopes": [{ "name": "local", "values": [{ "name": "x", "kind": "number", "preview": "1" }] }],
                "evaluated": { "name": "x", "kind": "number", "preview": "1", "children": 0 }
            },
            "output": ["hello"], "error": null
        });
        vec![
            (
                "egressRequest",
                "EgressRequest",
                via::<EgressRequest>(json!({
                    "method": "POST", "url": "https://api.example.com/v1", "headers": [["accept", "application/json"]],
                    "body": "{}", "extension": "acme", "purpose": "licence check", "collection": "shop"
                })),
            ),
            (
                "egressResponse",
                "EgressResponse",
                via::<EgressResponse>(json!({
                    "status": 200, "headers": [["content-type", "text/plain"]], "body": "ok", "truncated": false, "took_ms": 12
                })),
            ),
            (
                "egressHosts",
                "EgressHost[]",
                via::<Vec<EgressHost>>(json!([
                    { "host": "api.example.com", "port": 443, "extension": "acme", "purpose": "p", "allowed_at": 1_900_000_000 },
                    { "host": "x.example.com", "extension": "acme", "purpose": "p", "allowed_at": 1 }
                ])),
            ),
            (
                "egressErrors",
                "EgressError[]",
                via::<Vec<EgressError>>(json!([
                    { "kind": "not-allowed", "host": "a", "port": null },
                    { "kind": "blocked", "rule": "air-gap", "reason": "r" },
                    { "kind": "limit", "message": "m" },
                    { "kind": "transport", "message": "m" }
                ])),
            ),
            (
                "journalSettings",
                "JournalSettings",
                via::<JournalSettings>(json!({ "enabled": true, "id_path": "$.id" })),
            ),
            (
                "journalQuery",
                "JournalQuery",
                via::<JournalQuery>(
                    json!({ "limit": 50, "before": 9, "session": "s", "only_open": true }),
                ),
            ),
            (
                "journalPage",
                "JournalPage",
                via::<JournalPage>(json!({
                    "entries": [{
                        "id": 1, "session": "s", "at": 1_900_000_000, "environment": "Test", "request_path": "a.yml",
                        "method": "POST", "url": "https://x/orders", "status": 201, "location": "/orders/7",
                        "etag": "\"1\"", "resource_id": "7", "reverted_at": 1_900_000_100
                    }], "total": 1
                })),
            ),
            (
                "manualRevert",
                "ManualRevert",
                via::<ManualRevert>(json!({ "entry_id": 1, "request_path": "orders/delete.yml" })),
            ),
            (
                "compensationItems",
                "CompensationItem[]",
                via::<Vec<CompensationItem>>(json!([
                    { "entry_id": 1, "answer": { "kind": "compensation", "compensation": compensation.clone() } },
                    { "entry_id": 2, "answer": { "kind": "not-reversible", "reason": "r" } }
                ])),
            ),
            ("revertPlan", "RevertPlan", via::<RevertPlan>(plan.clone())),
            (
                "revertOptions",
                "RevertOptions",
                via::<RevertOptions>(json!({ "production_confirmed": true })),
            ),
            (
                "revertOutcome",
                "RevertOutcome",
                via::<RevertOutcome>(json!({ "steps": [
                { "entry_id": 1, "result": "reverted", "message": "ok" },
                { "entry_id": 2, "result": "skipped-conflict", "message": "changed" },
                { "entry_id": 3, "result": "skipped-not-reversible", "message": "n" },
                { "entry_id": 4, "result": "failed", "message": "f" }
            ] })),
            ),
            (
                "pluginCompensationRules",
                "PluginCompensationRule[]",
                via::<Vec<PluginCompensationRule>>(json!([])),
            ),
            (
                "entryStates",
                "EntryStates[]",
                via::<Vec<EntryStates>>(json!([
                    { "entry_id": 1, "before": { "status": 200, "body": "{}", "etag": "\"1\"" }, "after": null }
                ])),
            ),
            (
                "accessProfile",
                "AccessProfile",
                via::<AccessProfile>(json!({ "a": "alice", "b": "bob" })),
            ),
            (
                "accessOptions",
                "AccessOptions",
                via::<AccessOptions>(
                    json!({ "allow_writes": false, "b_must_be_denied": true, "rate": 5 }),
                ),
            ),
            ("accessCheck", "AccessCheck", via::<AccessCheck>(check)),
            (
                "shrinkRequest",
                "ShrinkRequest",
                via::<ShrinkRequest>(json!({
                    "path": "a.yml", "environment": "Test", "predicate": { "kind": "status", "status": 500 },
                    "keep": ["header:x-a"], "max_probes": 100, "max_seconds": 60, "simplify_values": true, "confirmed_writes": false
                })),
            ),
            (
                "shrinkPredicates",
                "ShrinkPredicate[]",
                json!([
                    { "kind": "status", "status": 500 }, { "kind": "server-error" },
                    { "kind": "assertion-fails", "assertion": "res.status eq 200" }, { "kind": "assertion-fails" },
                    { "kind": "body-matches", "pattern": "boom" }, { "kind": "header-present", "name": "x" },
                    { "kind": "header-missing", "name": "x" }, { "kind": "last-failure" }
                ]),
            ),
            ("shrinkResult", "ShrinkResult", via::<ShrinkResult>(shrink)),
            (
                "consumerRefs",
                "ConsumerRef[]",
                via::<Vec<ConsumerRef>>(json!([
                    { "name": "mobile", "source": "../mobile", "ref": "main", "path": "tests", "environment": "Test", "vars": [["k", "v"]] }
                ])),
            ),
            (
                "consumerVerify",
                "ConsumerVerifyRequest",
                via::<ConsumerVerifyRequest>(json!({
                    "base_url": "http://localhost:3000", "only": ["mobile"], "fetch": true, "keep_going": true,
                    "trust_scripts": false, "allow_writes": false, "max_minutes": 5
                })),
            ),
            (
                "consumerReport",
                "ConsumerReport",
                via::<ConsumerReport>(json!({ "consumers": [{
                "name": "mobile", "commit": "abc", "requests": 3, "skipped": 1,
                "failures": [{ "request_path": "a.yml", "request": "Get", "assertion": "res.status eq 200", "difference": "got 404" }],
                "error": null
            }] })),
            ),
            (
                "sequenceSources",
                "SequenceSource[]",
                via::<Vec<SequenceSource>>(json!([
                    { "kind": "run", "run_id": 1 }, { "kind": "session" }, { "kind": "pages", "request_path": "a.yml" },
                    { "kind": "mcp", "since": null }, { "kind": "history", "ids": [1, 2] }
                ])),
            ),
            (
                "sequenceOptions",
                "SequenceOptions",
                via::<SequenceOptions>(json!({ "include_query": true, "include_values": false })),
            ),
            (
                "sequenceModel",
                "SequenceModel",
                via::<SequenceModel>(model),
            ),
            (
                "sequenceText",
                "SequenceText",
                via::<SequenceText>(
                    json!({ "mermaid": "sequenceDiagram", "markdown": "```mermaid\n```" }),
                ),
            ),
            (
                "securityTarget",
                "SecurityTarget",
                via::<SecurityTarget>(json!({ "path": "a.yml", "response_id": 3 })),
            ),
            (
                "securityFindings",
                "SecurityFinding[]",
                via::<Vec<SecurityFinding>>(json!([{
                    "rule": "cors-wildcard-credentials", "category": "API8:2023", "severity": "high",
                    "location": "header:access-control-allow-origin", "why": "w", "fix": "f"
                }])),
            ),
            (
                "securityRules",
                "SecurityRule[]",
                via::<Vec<SecurityRule>>(json!([
                    { "id": "tls-expiry", "title": "t", "category": "API8:2023", "severity": "medium", "native_only": true },
                    { "id": "x", "title": "t", "category": "API1:2023", "severity": "info" }
                ])),
            ),
            (
                "securitySettings",
                "SecuritySettings",
                via::<SecuritySettings>(json!({ "enabled": true, "disabled": ["x"] })),
            ),
            (
                "tlsInfo",
                "TlsInfo",
                via::<TlsInfo>(json!({
                    "protocol": "TLSv1.3", "cipher": "TLS_AES_128_GCM_SHA256", "not_before": 1, "not_after": 2,
                    "key_algorithm": "rsa", "key_bits": 2048, "names": ["a.example.com"]
                })),
            ),
            (
                "traceRef",
                "TraceRef",
                via::<TraceRef>(json!({
                    "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736", "span_id": "00f067aa0ba902b7", "injected": true
                })),
            ),
            (
                "offsetSegment",
                "TimingSegment",
                via::<TimingSegment>(json!({
                    "id": "db", "label": "Database", "ms": 5, "parent": "server", "offset_ms": 1.5
                })),
            ),
            (
                "notebookEntries",
                "NotebookEntry[]",
                via::<Vec<NotebookEntry>>(
                    json!([{ "path": "a.notebook.md", "title": "Orders", "blocks": 3 }]),
                ),
            ),
            (
                "notebook",
                "Notebook",
                via::<Notebook>(json!({ "title": "Orders", "blocks": [
                { "kind": "text", "markdown": "# Orders" },
                { "kind": "request", "id": "list", "ref": "orders/list.yml", "set": [["n", "5"]],
                  "show": [{ "kind": "response" }, { "kind": "table", "path": "$.items" }, { "kind": "chart", "path": "$.items[*].n" }] },
                { "kind": "request", "id": "adhoc", "embedded": spec.clone() }
            ] })),
            ),
            (
                "notebookRun",
                "NotebookRun",
                via::<NotebookRun>(
                    json!({ "path": "a.notebook.md", "block": "list", "environment": "Test", "allow_writes": false }),
                ),
            ),
            (
                "notebookOutcome",
                "NotebookOutcome",
                via::<NotebookOutcome>(json!({ "results": [
                { "block": "list", "set": [["id", "7"]] }, { "block": "x", "skipped": "writes need confirmation" }
            ] })),
            ),
            (
                "debugTarget",
                "DebugTarget",
                via::<DebugTarget>(json!({
                    "request_path": "a.yml", "phase": "tests", "folder": "orders", "environment": "Test",
                    "breakpoints": [{ "line": 3, "condition": "x > 1" }], "stop_on_entry": true
                })),
            ),
            (
                "debugCommands",
                "DebugCommand[]",
                via::<Vec<DebugCommand>>(json!([
                    { "kind": "continue" }, { "kind": "step-over" }, { "kind": "step-into" }, { "kind": "step-out" },
                    { "kind": "evaluate", "expression": "x" },
                    { "kind": "set-breakpoints", "breakpoints": [{ "line": 1 }] }, { "kind": "abort" }
                ])),
            ),
            ("debugState", "DebugState", via::<DebugState>(debug_state)),
        ]
    }
}
