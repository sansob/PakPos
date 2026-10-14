//! gRPC client with dynamic messages: describe services from `.proto` files
//! (compiled in-process by `protox`) or from server reflection, then call any
//! method with JSON in and JSON out. Unary, server-streaming, client-streaming
//! and bidirectional calls are supported (streaming input is a JSON array that
//! is sent in full before reading responses).
//!
//! The functions here are blocking; each call runs a small Tokio runtime.

use crate::http::HttpSettings;
use crate::model::GrpcConfig;
use prost::Message as _;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor, MethodDescriptor, SerializeOptions};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use tonic::Status;

#[derive(Debug, thiserror::Error)]
pub enum GrpcError {
    #[error("invalid gRPC URL `{0}`")]
    Url(String),
    #[error("cannot connect: {0}")]
    Connect(String),
    #[error("proto files: {0}")]
    Proto(String),
    #[error("server reflection failed: {0} (add the service's .proto files instead)")]
    Reflection(String),
    #[error("method `{0}` not found")]
    NoMethod(String),
    #[error("invalid request message: {0}")]
    Message(String),
}

/// Normalises `grpc://`, `grpcs://`, `http(s)://` or bare `host:port` URLs.
/// Returns the HTTP/2 URL and whether TLS is used.
pub fn endpoint_url(url: &str) -> Result<(String, bool), GrpcError> {
    let u = url.trim();
    let (rest, tls) = if let Some(r) = u.strip_prefix("grpcs://").or_else(|| u.strip_prefix("https://")) {
        (r, true)
    } else if let Some(r) = u.strip_prefix("grpc://").or_else(|| u.strip_prefix("http://")) {
        (r, false)
    } else {
        (u, false)
    };
    let rest = rest.trim_end_matches('/');
    if rest.is_empty() {
        return Err(GrpcError::Url(url.to_string()));
    }
    let scheme_url = format!("{}://{rest}", if tls { "https" } else { "http" });
    let parsed = url::Url::parse(&scheme_url).map_err(|_| GrpcError::Url(url.to_string()))?;
    if parsed.host_str().is_none() {
        return Err(GrpcError::Url(url.to_string()));
    }
    // Only scheme://host:port is used; a path in the URL would be ignored by gRPC.
    let port = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();
    Ok((format!("{}://{}{port}", parsed.scheme(), parsed.host_str().unwrap_or("")), tls))
}

async fn connect(url: &str, settings: &HttpSettings) -> Result<Channel, GrpcError> {
    let (uri, tls) = endpoint_url(url)?;
    let mut ep = Endpoint::from_shared(uri.clone())
        .map_err(|e| GrpcError::Url(format!("{uri}: {e}")))?
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(settings.timeout_secs.max(1)));
    if tls {
        ep =
            ep.tls_config(ClientTlsConfig::new().with_native_roots()).map_err(|e| GrpcError::Connect(e.to_string()))?;
    }
    ep.connect().await.map_err(|e| {
        use std::error::Error;
        let mut msg = e.to_string();
        let mut src = e.source();
        while let Some(s) = src {
            msg.push_str(": ");
            msg.push_str(&s.to_string());
            src = s.source();
        }
        GrpcError::Connect(msg)
    })
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime")
}

// ---- descriptors ------------------------------------------------------------

/// Compiles `.proto` files into a descriptor pool.
pub fn pool_from_protos(files: &[String], import_paths: &[String]) -> Result<DescriptorPool, GrpcError> {
    if files.is_empty() {
        return Err(GrpcError::Proto("no .proto files given".into()));
    }
    let mut includes: Vec<PathBuf> = import_paths.iter().map(PathBuf::from).collect();
    for f in files {
        if let Some(parent) = Path::new(f).parent() {
            let parent = if parent.as_os_str().is_empty() { Path::new(".") } else { parent };
            if !includes.iter().any(|i| i == parent) {
                includes.push(parent.to_path_buf());
            }
        }
    }
    // protox resolves files relative to an include path; pass file names
    // relative to the include that contains them.
    let rel: Vec<PathBuf> = files
        .iter()
        .map(|f| {
            let p = Path::new(f);
            includes
                .iter()
                .find_map(|i| p.strip_prefix(i).ok().map(Path::to_path_buf))
                .unwrap_or_else(|| p.to_path_buf())
        })
        .collect();
    let fds = protox::compile(&rel, &includes).map_err(|e| GrpcError::Proto(e.to_string()))?;
    DescriptorPool::from_file_descriptor_set(fds).map_err(|e| GrpcError::Proto(e.to_string()))
}

macro_rules! reflect_impl {
    ($name:ident, $pb:path) => {
        async fn $name(channel: Channel) -> Result<Vec<prost_types::FileDescriptorProto>, Status> {
            use pb::server_reflection_client::ServerReflectionClient;
            use pb::server_reflection_request::MessageRequest;
            use pb::server_reflection_response::MessageResponse;
            use $pb as pb;

            let mut client = ServerReflectionClient::new(channel);
            async fn ask(
                client: &mut ServerReflectionClient<Channel>,
                req: MessageRequest,
            ) -> Result<MessageResponse, Status> {
                let request = pb::ServerReflectionRequest { host: String::new(), message_request: Some(req) };
                let mut stream = client.server_reflection_info(tokio_stream::iter(vec![request])).await?.into_inner();
                match stream.message().await? {
                    Some(r) => match r.message_response {
                        Some(MessageResponse::ErrorResponse(e)) => {
                            Err(Status::new(e.error_code.into(), e.error_message))
                        }
                        Some(m) => Ok(m),
                        None => Err(Status::internal("empty reflection response")),
                    },
                    None => Err(Status::internal("reflection stream closed")),
                }
            }

            let services = match ask(&mut client, MessageRequest::ListServices(String::new())).await? {
                MessageResponse::ListServicesResponse(l) => l.service.into_iter().map(|s| s.name).collect::<Vec<_>>(),
                _ => return Err(Status::internal("unexpected reflection response")),
            };
            let mut files: BTreeMap<String, prost_types::FileDescriptorProto> = BTreeMap::new();
            let add = |bytes: Vec<Vec<u8>>, files: &mut BTreeMap<String, prost_types::FileDescriptorProto>| {
                for b in bytes {
                    if let Ok(fd) = prost_types::FileDescriptorProto::decode(b.as_slice()) {
                        files.insert(fd.name().to_string(), fd);
                    }
                }
            };
            for svc in services.iter().filter(|s| !s.starts_with("grpc.reflection.")) {
                if let MessageResponse::FileDescriptorResponse(r) =
                    ask(&mut client, MessageRequest::FileContainingSymbol(svc.clone())).await?
                {
                    add(r.file_descriptor_proto, &mut files);
                }
            }
            // Fetch missing dependencies by file name.
            let mut tried = HashSet::new();
            loop {
                let missing: Vec<String> = files
                    .values()
                    .flat_map(|f| f.dependency.clone())
                    .filter(|d| !files.contains_key(d) && !tried.contains(d))
                    .collect();
                if missing.is_empty() {
                    break;
                }
                for dep in missing {
                    tried.insert(dep.clone());
                    if let Ok(MessageResponse::FileDescriptorResponse(r)) =
                        ask(&mut client, MessageRequest::FileByFilename(dep)).await
                    {
                        add(r.file_descriptor_proto, &mut files);
                    }
                }
            }
            Ok(files.into_values().collect())
        }
    };
}

reflect_impl!(reflect_v1, tonic_reflection::pb::v1);
reflect_impl!(reflect_v1alpha, tonic_reflection::pb::v1alpha);

/// Builds a pool from descriptors in dependency order, filling in well-known
/// types (`google/protobuf/*.proto`) the server didn't send.
fn pool_from_files(mut files: Vec<prost_types::FileDescriptorProto>) -> Result<DescriptorPool, GrpcError> {
    let known: HashSet<String> = files.iter().map(|f| f.name().to_string()).collect();
    for f in DescriptorPool::global().files() {
        let name = f.name().to_string();
        if !known.contains(&name) && files.iter().any(|x| x.dependency.contains(&name)) {
            files.push(f.file_descriptor_proto().clone());
        }
    }
    let mut pool = DescriptorPool::new();
    let mut added: HashSet<String> = HashSet::new();
    while !files.is_empty() {
        let before = files.len();
        let mut rest = Vec::new();
        for f in files {
            if f.dependency.iter().all(|d| added.contains(d)) {
                added.insert(f.name().to_string());
                pool.add_file_descriptor_proto(f).map_err(|e| GrpcError::Reflection(e.to_string()))?;
            } else {
                rest.push(f);
            }
        }
        if rest.len() == before {
            let names: Vec<_> = rest.iter().map(|f| f.name().to_string()).collect();
            return Err(GrpcError::Reflection(format!("unresolved imports in {}", names.join(", "))));
        }
        files = rest;
    }
    Ok(pool)
}

/// Describes the server: from `.proto` files when configured, otherwise via reflection.
pub fn load_pool(cfg: &GrpcConfig, url: &str, settings: &HttpSettings) -> Result<DescriptorPool, GrpcError> {
    if !cfg.proto_files.is_empty() {
        return pool_from_protos(&cfg.proto_files, &cfg.import_paths);
    }
    runtime().block_on(async {
        let channel = connect(url, settings).await?;
        let files = match reflect_v1(channel.clone()).await {
            Ok(f) => f,
            Err(s) if s.code() == tonic::Code::Unimplemented => {
                reflect_v1alpha(channel).await.map_err(|s| GrpcError::Reflection(s.message().to_string()))?
            }
            Err(s) => return Err(GrpcError::Reflection(format!("{:?}: {}", s.code(), s.message()))),
        };
        pool_from_files(files)
    })
}

/// A callable method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodInfo {
    /// `package.Service/Method`
    pub full_name: String,
    pub service: String,
    pub method: String,
    pub client_streaming: bool,
    pub server_streaming: bool,
    pub input: String,
    pub output: String,
}

impl MethodInfo {
    pub fn kind_label(&self) -> &'static str {
        match (self.client_streaming, self.server_streaming) {
            (false, false) => "unary",
            (false, true) => "server streaming",
            (true, false) => "client streaming",
            (true, true) => "bidirectional",
        }
    }
}

pub fn methods(pool: &DescriptorPool) -> Vec<MethodInfo> {
    let mut out = Vec::new();
    for svc in pool.services().filter(|s| !s.full_name().starts_with("grpc.reflection.")) {
        for m in svc.methods() {
            out.push(MethodInfo {
                full_name: format!("{}/{}", svc.full_name(), m.name()),
                service: svc.full_name().to_string(),
                method: m.name().to_string(),
                client_streaming: m.is_client_streaming(),
                server_streaming: m.is_server_streaming(),
                input: m.input().full_name().to_string(),
                output: m.output().full_name().to_string(),
            });
        }
    }
    out
}

fn find_method(pool: &DescriptorPool, full: &str) -> Result<MethodDescriptor, GrpcError> {
    let (svc, name) =
        full.trim().trim_start_matches('/').split_once('/').ok_or_else(|| GrpcError::NoMethod(full.into()))?;
    pool.get_service_by_name(svc)
        .and_then(|s| s.methods().find(|m| m.name() == name))
        .ok_or_else(|| GrpcError::NoMethod(full.into()))
}

fn template_value(desc: &MessageDescriptor, depth: usize) -> serde_json::Value {
    use prost_reflect::Kind;
    let mut obj = serde_json::Map::new();
    for f in desc.fields() {
        let v = match f.kind() {
            Kind::Message(m) if depth < 3 && !m.full_name().starts_with("google.protobuf.") => {
                template_value(&m, depth + 1)
            }
            Kind::Message(_) => serde_json::Value::Null,
            Kind::String => serde_json::Value::String(String::new()),
            Kind::Bool => serde_json::Value::Bool(false),
            Kind::Bytes => serde_json::Value::String(String::new()),
            Kind::Enum(e) => {
                serde_json::Value::String(e.values().next().map(|v| v.name().to_string()).unwrap_or_default())
            }
            Kind::Double | Kind::Float => serde_json::json!(0.0),
            Kind::Int64 | Kind::Uint64 | Kind::Sint64 | Kind::Fixed64 | Kind::Sfixed64 => serde_json::json!("0"),
            _ => serde_json::json!(0),
        };
        let v = if f.is_list() {
            serde_json::Value::Array(vec![v])
        } else if f.is_map() {
            serde_json::json!({})
        } else {
            v
        };
        obj.insert(f.json_name().to_string(), v);
    }
    serde_json::Value::Object(obj)
}

/// A JSON skeleton of the method's input message (an array for client streaming).
pub fn request_template(pool: &DescriptorPool, full_method: &str) -> Result<String, GrpcError> {
    let m = find_method(pool, full_method)?;
    let v = template_value(&m.input(), 0);
    let v = if m.is_client_streaming() { serde_json::Value::Array(vec![v]) } else { v };
    Ok(serde_json::to_string_pretty(&v).unwrap_or_default())
}

// ---- calls ------------------------------------------------------------------

#[derive(Clone)]
struct DynamicCodec(MessageDescriptor);

impl Codec for DynamicCodec {
    type Encode = DynamicMessage;
    type Decode = DynamicMessage;
    type Encoder = DynamicCodec;
    type Decoder = DynamicCodec;
    fn encoder(&mut self) -> Self::Encoder {
        self.clone()
    }
    fn decoder(&mut self) -> Self::Decoder {
        self.clone()
    }
}

impl Encoder for DynamicCodec {
    type Item = DynamicMessage;
    type Error = Status;
    fn encode(&mut self, item: Self::Item, dst: &mut EncodeBuf<'_>) -> Result<(), Self::Error> {
        item.encode(dst).map_err(|e| Status::internal(e.to_string()))
    }
}

impl Decoder for DynamicCodec {
    type Item = DynamicMessage;
    type Error = Status;
    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<Self::Item>, Self::Error> {
        DynamicMessage::decode(self.0.clone(), src).map(Some).map_err(|e| Status::internal(e.to_string()))
    }
}

/// Result of a call.
#[derive(Debug, Clone, Default)]
pub struct GrpcResponse {
    /// gRPC status code (0 = OK).
    pub code: i32,
    pub code_name: String,
    pub status_message: String,
    /// Response messages as pretty JSON (several for server streaming).
    pub messages: Vec<String>,
    pub headers: Vec<(String, String)>,
    pub trailers: Vec<(String, String)>,
    pub elapsed_ms: u128,
}

impl GrpcResponse {
    pub fn is_ok(&self) -> bool {
        self.code == 0
    }

    /// The single response, or all of them as a JSON array.
    pub fn body(&self) -> String {
        match self.messages.len() {
            1 => self.messages[0].clone(),
            _ => format!("[\n{}\n]", self.messages.join(",\n")),
        }
    }
}

fn to_json(msg: &DynamicMessage) -> String {
    let mut ser = serde_json::Serializer::pretty(Vec::new());
    let opts = SerializeOptions::new().skip_default_fields(false);
    match msg.serialize_with_options(&mut ser, &opts) {
        Ok(()) => String::from_utf8(ser.into_inner()).unwrap_or_default(),
        Err(e) => format!("<cannot render message: {e}>"),
    }
}

fn metadata_pairs(md: &tonic::metadata::MetadataMap) -> Vec<(String, String)> {
    md.iter()
        .map(|kv| match kv {
            tonic::metadata::KeyAndValueRef::Ascii(k, v) => (k.to_string(), v.to_str().unwrap_or("").to_string()),
            tonic::metadata::KeyAndValueRef::Binary(k, v) => {
                (k.to_string(), format!("<{} bytes>", v.as_encoded_bytes().len()))
            }
        })
        .collect()
}

fn parse_message(desc: &MessageDescriptor, value: serde_json::Value) -> Result<DynamicMessage, GrpcError> {
    DynamicMessage::deserialize(desc.clone(), value).map_err(|e| GrpcError::Message(e.to_string()))
}

/// Calls `full_method` with a JSON message. `metadata` are sent as request
/// metadata (`authorization`, custom headers). At most `max_messages`
/// streamed responses are collected.
pub fn call(
    url: &str,
    pool: &DescriptorPool,
    full_method: &str,
    request_json: &str,
    metadata: &[(String, String)],
    settings: &HttpSettings,
    max_messages: usize,
) -> Result<GrpcResponse, GrpcError> {
    let method = find_method(pool, full_method)?;
    let json: serde_json::Value = if request_json.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(request_json).map_err(|e| GrpcError::Message(format!("not valid JSON: {e}")))?
    };
    let input = method.input();
    let messages: Vec<DynamicMessage> = if method.is_client_streaming() {
        let items = match json {
            serde_json::Value::Array(a) => a,
            other => vec![other],
        };
        items.into_iter().map(|v| parse_message(&input, v)).collect::<Result<_, _>>()?
    } else {
        vec![parse_message(&input, json)?]
    };
    let path: tonic::codegen::http::uri::PathAndQuery =
        format!("/{}/{}", method.parent_service().full_name(), method.name())
            .parse()
            .map_err(|_| GrpcError::NoMethod(full_method.into()))?;

    runtime().block_on(async move {
        let channel = connect(url, settings).await?;
        let mut grpc = tonic::client::Grpc::new(channel);
        grpc.ready().await.map_err(|e| GrpcError::Connect(e.to_string()))?;
        let codec = DynamicCodec(method.output());
        let started = Instant::now();
        let add_md = |md: &mut tonic::metadata::MetadataMap| {
            for (k, v) in metadata {
                let k = k.trim().to_ascii_lowercase();
                if k.is_empty() || k == "content-type" || k == "user-agent" || k.ends_with("-bin") {
                    continue;
                }
                if let (Ok(key), Ok(val)) = (
                    tonic::metadata::MetadataKey::<tonic::metadata::Ascii>::from_bytes(k.as_bytes()),
                    tonic::metadata::MetadataValue::try_from(v.as_str()),
                ) {
                    md.insert(key, val);
                }
            }
        };
        let mut out = GrpcResponse::default();
        let status_of = |out: &mut GrpcResponse, s: &Status| {
            out.code = s.code() as i32;
            out.code_name = format!("{:?}", s.code());
            out.status_message = s.message().to_string();
            out.trailers.extend(metadata_pairs(s.metadata()));
        };
        let finish_stream = |out: &mut GrpcResponse, resp: tonic::Response<tonic::Streaming<DynamicMessage>>| {
            out.headers = metadata_pairs(resp.metadata());
            resp.into_inner()
        };
        let streaming = method.is_client_streaming() || method.is_server_streaming();
        if !streaming {
            let mut req = tonic::Request::new(messages.into_iter().next().expect("one message"));
            add_md(req.metadata_mut());
            match grpc.unary(req, path, codec).await {
                Ok(resp) => {
                    out.headers = metadata_pairs(resp.metadata());
                    out.messages.push(to_json(resp.get_ref()));
                    out.code_name = "Ok".into();
                }
                Err(s) => status_of(&mut out, &s),
            }
        } else {
            let result = if method.is_client_streaming() {
                let mut req = tonic::Request::new(tokio_stream::iter(messages));
                add_md(req.metadata_mut());
                if method.is_server_streaming() {
                    grpc.streaming(req, path, codec).await.map(|r| finish_stream(&mut out, r))
                } else {
                    match grpc.client_streaming(req, path, codec).await {
                        Ok(resp) => {
                            out.headers = metadata_pairs(resp.metadata());
                            out.messages.push(to_json(resp.get_ref()));
                            out.code_name = "Ok".into();
                            out.elapsed_ms = started.elapsed().as_millis();
                            return Ok(out);
                        }
                        Err(s) => Err(s),
                    }
                }
            } else {
                let mut req = tonic::Request::new(messages.into_iter().next().expect("one message"));
                add_md(req.metadata_mut());
                grpc.server_streaming(req, path, codec).await.map(|r| finish_stream(&mut out, r))
            };
            match result {
                Ok(mut stream) => {
                    out.code_name = "Ok".into();
                    loop {
                        if out.messages.len() >= max_messages {
                            out.status_message = format!("stopped after {max_messages} messages");
                            break;
                        }
                        match stream.message().await {
                            Ok(Some(m)) => out.messages.push(to_json(&m)),
                            Ok(None) => {
                                if let Ok(Some(t)) = stream.trailers().await {
                                    out.trailers = metadata_pairs(&t);
                                }
                                break;
                            }
                            Err(s) => {
                                status_of(&mut out, &s);
                                break;
                            }
                        }
                    }
                }
                Err(s) => status_of(&mut out, &s),
            }
        }
        out.elapsed_ms = started.elapsed().as_millis();
        Ok(out)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_urls() {
        assert_eq!(endpoint_url("grpc://localhost:50051").unwrap(), ("http://localhost:50051".into(), false));
        assert_eq!(endpoint_url("grpcs://api.example.com").unwrap(), ("https://api.example.com".into(), true));
        assert_eq!(endpoint_url("localhost:9000/").unwrap(), ("http://localhost:9000".into(), false));
        // The default port is implicit; the path is dropped.
        assert_eq!(endpoint_url("https://h:443/ignored/path").unwrap(), ("https://h".into(), true));
        assert!(endpoint_url("grpc://").is_err());
    }

    #[test]
    fn compiles_protos_and_builds_templates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shop.proto");
        std::fs::write(
            &path,
            r#"syntax = "proto3";
package shop.v1;
import "google/protobuf/timestamp.proto";
enum Status { STATUS_UNSPECIFIED = 0; PAID = 1; }
message Item { string sku = 1; int32 qty = 2; }
message OrderRequest { string customer = 1; repeated Item items = 2; Status status = 3; google.protobuf.Timestamp at = 4; }
message OrderReply { string id = 1; int64 total = 2; }
service Orders {
  rpc Create(OrderRequest) returns (OrderReply);
  rpc Watch(OrderRequest) returns (stream OrderReply);
  rpc Upload(stream Item) returns (OrderReply);
}"#,
        )
        .unwrap();
        let pool = pool_from_protos(&[path.display().to_string()], &[]).unwrap();
        let ms = methods(&pool);
        let names: Vec<_> = ms.iter().map(|m| (m.full_name.as_str(), m.kind_label())).collect();
        assert_eq!(
            names,
            vec![
                ("shop.v1.Orders/Create", "unary"),
                ("shop.v1.Orders/Watch", "server streaming"),
                ("shop.v1.Orders/Upload", "client streaming")
            ]
        );
        let t: serde_json::Value =
            serde_json::from_str(&request_template(&pool, "shop.v1.Orders/Create").unwrap()).unwrap();
        assert_eq!(t["items"][0]["qty"], 0);
        assert_eq!(t["status"], "STATUS_UNSPECIFIED");
        assert!(request_template(&pool, "shop.v1.Orders/Upload").unwrap().trim_start().starts_with('['));
        assert!(matches!(request_template(&pool, "shop.v1.Orders/Nope"), Err(GrpcError::NoMethod(_))));
        // JSON → message validation happens before any network traffic.
        let err = call(
            "grpc://127.0.0.1:1",
            &pool,
            "shop.v1.Orders/Create",
            r#"{"qty_typo": 1}"#,
            &[],
            &HttpSettings::default(),
            10,
        );
        assert!(matches!(err, Err(GrpcError::Message(_))), "{err:?}");
    }
}
