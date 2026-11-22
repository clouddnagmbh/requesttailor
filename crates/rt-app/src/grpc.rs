// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! gRPC services and message templates for the app (wave 8, strand B; `GrpcApi`).
//!
//! The descriptors of a request come from its `protoFilePath`, else the collection's
//! `config.protobuf`, else server reflection ([`rt_grpc::proto_source`]). Proto files are read
//! from the workspace's file set (every file of the opened folder), so desktop and web resolve
//! them the same way; a file outside the collection folder is "not found".
//!
//! [`GrpcSnapshot`] is owned, so the shells await reflection without borrowing the
//! `Workspace`. Reflection is sent to the request's resolved URL with its resolved metadata
//! (headers and static auth from `rt_core::prepare`; OAuth 2.0 tokens and plugin hooks are not
//! applied to reflection calls).

use rt_core::model::{GrpcMethodType, GrpcSpec, Protocol};
use rt_core::prepare::{PrepareInput, prepare};
use rt_core::stream::{Frame, StreamEvent, StreamTransport};
use rt_format::FileSet;
pub use rt_grpc::{Descriptors, GrpcError, GrpcMethod, GrpcService, ProtoSource};

use crate::{AppError, SendInput, Workspace};

impl From<GrpcError> for AppError {
    fn from(e: GrpcError) -> Self {
        match e {
            GrpcError::Proto(m) => AppError::Format(m),
            other => AppError::Shell(other.to_string()),
        }
    }
}

/// Everything needed to get the descriptors of one gRPC request.
#[derive(Debug, Clone)]
pub struct GrpcSnapshot {
    pub spec: GrpcSpec,
    pub source: ProtoSource,
    /// Proto sources (all `.proto` files of the collection) for [`ProtoSource::Files`].
    files: FileSet,
    /// Resolved URL and metadata for reflection (with secret values; never shown).
    pub url: String,
    pub metadata: Vec<(String, String)>,
}

/// The gRPC request at `path`: where its descriptors come from, and its resolved URL and
/// metadata. Fails if the request is not a gRPC request.
pub fn snapshot(ws: &Workspace, path: &str, input: SendInput) -> Result<GrpcSnapshot, AppError> {
    let send = ws.snapshot(path, input)?;
    let context = send
        .collection
        .request(path)
        .ok_or_else(|| AppError::RequestNotFound(path.to_owned()))?;
    let Protocol::Grpc(spec) = &context.request.spec.protocol else {
        return Err(AppError::Unsupported(format!(
            "{path} is not a gRPC request"
        )));
    };
    let source = rt_grpc::proto_source(&send.collection.config.protobuf, path, spec);
    let (url, metadata) = match &source {
        ProtoSource::Files { .. } => (String::new(), Vec::new()),
        ProtoSource::Reflection => {
            let prepared = prepare(
                &context,
                &PrepareInput {
                    environment: send.environment.as_deref(),
                    globals: Vec::new(),
                    runtime: send.vars.clone(),
                },
                &crate::workspace::MemFiles(&send.files),
            )
            .map_err(|e| AppError::Shell(e.to_string()))?;
            let metadata = prepared
                .request
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            (prepared.request.url.clone(), metadata)
        }
    };
    let files = FileSet {
        files: ws
            .files()
            .files
            .iter()
            .filter(|(p, _)| p.ends_with(".proto"))
            .map(|(p, b)| (p.clone(), b.clone()))
            .collect(),
    };
    Ok(GrpcSnapshot {
        spec: (**spec).clone(),
        source,
        files,
        url,
        metadata,
    })
}

impl GrpcSnapshot {
    /// Compiles the proto files, or asks the server through `transport`.
    pub async fn descriptors<S: StreamTransport + ?Sized>(
        &self,
        transport: &S,
    ) -> Result<Descriptors, AppError> {
        match &self.source {
            ProtoSource::Files {
                files,
                import_paths,
            } => Ok(self.compile_files(files, import_paths)?),
            ProtoSource::Reflection => {
                Ok(Descriptors::from_reflection(transport, &self.url, &self.metadata).await?)
            }
        }
    }

    /// The proto files only; `None` if the request needs server reflection.
    pub fn local_descriptors(&self) -> Option<Result<Descriptors, AppError>> {
        match &self.source {
            ProtoSource::Files {
                files,
                import_paths,
            } => Some(self.compile_files(files, import_paths).map_err(Into::into)),
            ProtoSource::Reflection => None,
        }
    }

    fn compile_files(
        &self,
        files: &[String],
        import_paths: &[String],
    ) -> Result<Descriptors, GrpcError> {
        Descriptors::from_loader(files, import_paths, &|p: &str| {
            self.files
                .files
                .get(p)
                .map(|b| String::from_utf8_lossy(b).into_owned())
        })
    }
}

/// The protobuf codec of an open gRPC session (desktop and web sessions): sent JSON
/// messages are encoded with the method's input type, and events are decoded for display
/// (received: output type, sent: input type) before they reach the log and the webview.
#[derive(Debug, Clone)]
pub struct SessionCodec {
    descriptors: Descriptors,
    method: String,
    /// Unary and server-streaming calls take one message: the shells half-close after it.
    single: bool,
}

impl SessionCodec {
    /// The descriptors of `snapshot` (proto files or reflection); the method must be one of
    /// them.
    pub async fn open<S: StreamTransport + ?Sized>(
        snapshot: &GrpcSnapshot,
        transport: &S,
    ) -> Result<Self, AppError> {
        let descriptors = snapshot.descriptors(transport).await?;
        descriptors.method(&snapshot.spec.method)?;
        Ok(Self::new(descriptors, snapshot))
    }

    /// The codec of `snapshot`'s method with descriptors already in hand.
    pub fn new(descriptors: Descriptors, snapshot: &GrpcSnapshot) -> Self {
        let method = snapshot.spec.method.clone();
        let method_type = descriptors
            .method(&method)
            .map(|m| m.method_type)
            .unwrap_or(snapshot.spec.method_type);
        Self {
            descriptors,
            method,
            single: matches!(
                method_type,
                GrpcMethodType::Unary | GrpcMethodType::ServerStreaming
            ),
        }
    }

    /// Whether the call ends its client side after one message (unary, server streaming).
    pub fn ends_after_send(&self) -> bool {
        self.single
    }

    /// A message frame (variables resolved) as protobuf.
    pub fn encode(&self, frame: Frame) -> Result<Frame, AppError> {
        match frame {
            Frame::Text(json) => Ok(self.descriptors.encode_frame(&self.method, &json)?),
            Frame::Binary(_) => Err(AppError::Format(
                "gRPC messages are JSON; binary messages are not supported".into(),
            )),
        }
    }

    /// `event` with protobuf messages as JSON text.
    pub fn decode(&self, event: StreamEvent) -> StreamEvent {
        self.descriptors.decode_event(&self.method, event)
    }
}

/// `GrpcApi.services`.
pub async fn services<S: StreamTransport + ?Sized>(
    snapshot: &GrpcSnapshot,
    transport: &S,
) -> Result<Vec<GrpcService>, AppError> {
    Ok(snapshot.descriptors(transport).await?.services())
}

/// `GrpcApi.template`.
pub async fn template<S: StreamTransport + ?Sized>(
    snapshot: &GrpcSnapshot,
    method: &str,
    transport: &S,
) -> Result<String, AppError> {
    Ok(snapshot.descriptors(transport).await?.template(method)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_core::model::GrpcMethodType;

    const ROOT: &str = "opencollection: 1.0.0\ninfo:\n  name: g\nconfig:\n  protobuf:\n    protoFiles:\n      - path: protos/echo.proto\n    importPaths:\n      - path: include\n";
    const REQ: &str = "info:\n  name: echo\n  type: grpc\n\ngrpc:\n  url: \"{{host}}\"\n  method: /rt.echo.v1.EchoService/Echo\n  methodType: unary\n";
    const OWN: &str = "info:\n  name: own\n  type: grpc\n\ngrpc:\n  url: localhost:1\n  method: /x.Y/Z\n  methodType: unary\n  protoFilePath: ../protos/own.proto\n";
    const HTTP: &str = "info:\n  name: h\n  type: http\n\nhttp:\n  method: GET\n  url: https://x\n";

    fn testdata(name: &str) -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/grpc")
                .join(name),
        )
        .unwrap()
    }

    fn workspace(root: &str) -> Workspace {
        let mut files = FileSet::default();
        for (p, t) in [
            ("opencollection.yml", root),
            ("echo.yml", REQ),
            ("sub/own.yml", OWN),
            ("h.yml", HTTP),
            (
                "protos/own.proto",
                "syntax = \"proto3\";\npackage x;\nmessage M { string a = 1; }\nservice Y { rpc Z(M) returns (M); }\n",
            ),
        ] {
            files.files.insert(p.into(), t.as_bytes().to_vec());
        }
        files
            .files
            .insert("protos/echo.proto".into(), testdata("protos/echo.proto"));
        files.files.insert(
            "include/rt/common.proto".into(),
            testdata("include/rt/common.proto"),
        );
        Workspace::open(files).unwrap()
    }

    #[test]
    fn collection_and_request_proto_files() {
        let ws = workspace(ROOT);
        let snap = snapshot(&ws, "echo.yml", SendInput::default()).unwrap();
        let d = snap.local_descriptors().unwrap().unwrap();
        let services = d.services();
        assert_eq!(services[0].name, "rt.echo.v1.EchoService");
        let chat = services[0]
            .methods
            .iter()
            .find(|m| m.name == "Chat")
            .unwrap();
        assert_eq!(chat.method_type, GrpcMethodType::BidiStreaming);
        assert!(
            d.template("/rt.echo.v1.EchoService/Echo")
                .unwrap()
                .contains("\"message\": \"\"")
        );

        let own = snapshot(&ws, "sub/own.yml", SendInput::default()).unwrap();
        let names: Vec<String> = own
            .local_descriptors()
            .unwrap()
            .unwrap()
            .services()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["x.Y"]);
    }

    #[test]
    fn reflection_without_proto_files_and_errors() {
        let ws = workspace("opencollection: 1.0.0\ninfo:\n  name: g\n");
        let input = SendInput {
            vars: vec![("host".into(), "grpc://localhost:9".into())],
            ..SendInput::default()
        };
        let snap = snapshot(&ws, "echo.yml", input).unwrap();
        assert_eq!(snap.source, ProtoSource::Reflection);
        assert!(snap.local_descriptors().is_none());
        assert_eq!(snap.url, "grpc://localhost:9");
        assert!(matches!(
            snapshot(&ws, "h.yml", SendInput::default()),
            Err(AppError::Unsupported(_))
        ));
        assert!(matches!(
            snapshot(&ws, "nope.yml", SendInput::default()),
            Err(AppError::RequestNotFound(_))
        ));
    }

    #[test]
    fn a_broken_proto_file_is_a_format_error() {
        let mut files = FileSet::default();
        files
            .files
            .insert("opencollection.yml".into(), ROOT.as_bytes().to_vec());
        files
            .files
            .insert("echo.yml".into(), REQ.as_bytes().to_vec());
        files.files.insert(
            "protos/echo.proto".into(),
            b"syntax = \"proto3\";\nmessage {".to_vec(),
        );
        let ws = Workspace::open(files).unwrap();
        let snap = snapshot(&ws, "echo.yml", SendInput::default()).unwrap();
        let err = snap.local_descriptors().unwrap().unwrap_err();
        assert!(
            matches!(&err, AppError::Format(m) if m.starts_with("protos/echo.proto:2")),
            "{err:?}"
        );
    }
}
