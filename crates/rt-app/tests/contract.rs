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
    ApprovalDecision, AuditEntry, AuditOutcome, Effect, PendingApproval, Verdict,
};
use rt_app::ai::{AiAnswer, AiConfig, AiError, AiProviderKind, AiTask};
use rt_app::auth::{AuthPrompt, DeviceCodePrompt, TokenInfo};
use rt_app::cookies::CookieInfo;
use rt_app::git::{
    DiffHunk, DiffLine, DiffLineKind, GitBranch, GitCommit, GitConflict, GitDiff, GitFile,
    GitFileState, GitIdentity, GitPullResult, GitResolution, GitStatus,
};
use rt_app::mcp_client::{
    McpCallResult, McpContent, McpServerConfig, McpServerInfo, McpTest, McpTestResult, McpTool,
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
