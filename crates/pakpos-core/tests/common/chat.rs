//! `pakpos.demo.Chat`: a bidirectional `Talk` and a client-streaming `Sum`,
//! built from dynamic messages (no generated code). Shared by the gRPC demo
//! server example and the integration tests.

use pakpos_core::grpc::DynamicCodec;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor, Value};
use std::convert::Infallible;
use tonic::codegen::{http, BoxFuture, Context, Poll, Service};
use tonic::{Status, Streaming};

pub const PROTO: &str = r#"syntax = "proto3";
package pakpos.demo;

message ChatMessage {
  string user = 1;
  string text = 2;
}

message Number { int64 value = 1; }
message Total { int64 sum = 1; int32 count = 2; }

service Chat {
  // Answers every message right away, until the client ends the stream.
  rpc Talk(stream ChatMessage) returns (stream ChatMessage);
  // Adds up the numbers it receives.
  rpc Sum(stream Number) returns (Total);
}
"#;

pub fn pool() -> DescriptorPool {
    // One file per call: tests run in parallel, and rewriting a shared file
    // while another test compiles it gives an empty pool.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("pakpos-chat-proto-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let file = dir.join("chat.proto");
    std::fs::write(&file, PROTO).expect("write proto");
    pakpos_core::grpc::pool_from_protos(&[file.display().to_string()], &[]).expect("chat.proto compiles")
}

#[derive(Clone)]
pub struct ChatServer {
    pool: DescriptorPool,
}

impl ChatServer {
    pub fn new(pool: DescriptorPool) -> Self {
        Self { pool }
    }

    fn desc(&self, name: &str) -> MessageDescriptor {
        self.pool.get_message_by_name(name).expect("message in chat.proto")
    }
}

impl tonic::server::NamedService for ChatServer {
    const NAME: &'static str = "pakpos.demo.Chat";
}

struct Talk(MessageDescriptor);

impl Service<tonic::Request<Streaming<DynamicMessage>>> for Talk {
    type Response = tonic::Response<tonic::codegen::BoxStream<DynamicMessage>>;
    type Error = Status;
    type Future = BoxFuture<Self::Response, Status>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Status>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: tonic::Request<Streaming<DynamicMessage>>) -> Self::Future {
        let desc = self.0.clone();
        Box::pin(async move {
            let mut inbound = req.into_inner();
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(async move {
                let mut n = 0;
                while let Ok(Some(m)) = inbound.message().await {
                    n += 1;
                    let user =
                        m.get_field_by_name("user").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
                    let text =
                        m.get_field_by_name("text").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
                    let mut reply = DynamicMessage::new(desc.clone());
                    reply.set_field_by_name("user", Value::String("pakpos".into()));
                    reply.set_field_by_name("text", Value::String(format!("#{n} halo {user}: {text}")));
                    if tx.send(Ok(reply)).is_err() {
                        break;
                    }
                }
            });
            let stream: tonic::codegen::BoxStream<DynamicMessage> =
                Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(rx));
            Ok(tonic::Response::new(stream))
        })
    }
}

struct Sum(MessageDescriptor);

impl Service<tonic::Request<Streaming<DynamicMessage>>> for Sum {
    type Response = tonic::Response<DynamicMessage>;
    type Error = Status;
    type Future = BoxFuture<Self::Response, Status>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Status>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: tonic::Request<Streaming<DynamicMessage>>) -> Self::Future {
        let desc = self.0.clone();
        Box::pin(async move {
            let mut inbound = req.into_inner();
            let (mut sum, mut count) = (0i64, 0i32);
            while let Some(m) = inbound.message().await? {
                sum += m.get_field_by_name("value").and_then(|v| v.as_i64()).unwrap_or(0);
                count += 1;
            }
            let mut total = DynamicMessage::new(desc);
            total.set_field_by_name("sum", Value::I64(sum));
            total.set_field_by_name("count", Value::I32(count));
            Ok(tonic::Response::new(total))
        })
    }
}

impl Service<http::Request<tonic::body::Body>> for ChatServer {
    type Response = http::Response<tonic::body::Body>;
    type Error = Infallible;
    type Future = BoxFuture<Self::Response, Infallible>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<tonic::body::Body>) -> Self::Future {
        let this = self.clone();
        Box::pin(async move {
            Ok(match req.uri().path() {
                "/pakpos.demo.Chat/Talk" => {
                    let codec = DynamicCodec(this.desc("pakpos.demo.ChatMessage"));
                    tonic::server::Grpc::new(codec).streaming(Talk(this.desc("pakpos.demo.ChatMessage")), req).await
                }
                "/pakpos.demo.Chat/Sum" => {
                    let codec = DynamicCodec(this.desc("pakpos.demo.Number"));
                    tonic::server::Grpc::new(codec).client_streaming(Sum(this.desc("pakpos.demo.Total")), req).await
                }
                _ => Status::unimplemented("no such method").into_http(),
            })
        })
    }
}
