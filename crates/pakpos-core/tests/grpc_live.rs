//! gRPC client against a real tonic server (health + reflection services).

use pakpos_core::grpc;
use pakpos_core::http::HttpSettings;
use pakpos_core::model::GrpcConfig;

fn start_server() -> String {
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let (_reporter, health) = tonic_health::server::health_reporter();
            let reflection = tonic_reflection::server::Builder::configure()
                .register_encoded_file_descriptor_set(tonic_health::pb::FILE_DESCRIPTOR_SET)
                .build_v1()
                .unwrap();
            tonic::transport::Server::builder().add_service(health).add_service(reflection).serve(addr).await.unwrap();
        });
    });
    // Wait until it accepts connections.
    for _ in 0..50 {
        if std::net::TcpStream::connect(addr).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    format!("grpc://{addr}")
}

#[test]
fn reflection_unary_streaming_and_errors() {
    let url = start_server();
    let settings = HttpSettings::default();
    let pool = grpc::load_pool(&GrpcConfig::default(), &url, &settings).expect("reflection");
    let methods: Vec<String> = grpc::methods(&pool).into_iter().map(|m| m.full_name).collect();
    assert!(methods.contains(&"grpc.health.v1.Health/Check".to_string()), "{methods:?}");
    assert!(methods.contains(&"grpc.health.v1.Health/Watch".to_string()));

    let ok = grpc::call(
        &url,
        &pool,
        "grpc.health.v1.Health/Check",
        r#"{"service": ""}"#,
        &[("x-trace".into(), "1".into())],
        &settings,
        10,
    )
    .unwrap();
    assert!(ok.is_ok(), "{ok:?}");
    assert!(ok.messages[0].contains("SERVING"), "{}", ok.messages[0]);

    let missing =
        grpc::call(&url, &pool, "grpc.health.v1.Health/Check", r#"{"service": "nope"}"#, &[], &settings, 10).unwrap();
    assert_eq!(missing.code, 5, "NOT_FOUND: {missing:?}");
    assert_eq!(missing.code_name, "NotFound");

    // Watch streams forever; stop after the first message.
    let watch =
        grpc::call(&url, &pool, "grpc.health.v1.Health/Watch", r#"{"service": ""}"#, &[], &settings, 1).unwrap();
    assert_eq!(watch.messages.len(), 1);
    assert!(watch.status_message.contains("stopped after 1"));

    let tpl = grpc::request_template(&pool, "grpc.health.v1.Health/Check").unwrap();
    assert!(tpl.contains("\"service\""));
}

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/mtls/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// A TLS server that requires a client certificate signed by the test CA.
fn start_mtls_server() -> String {
    use tonic::transport::{Certificate, Identity, ServerTlsConfig};
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let read = |n: &str| std::fs::read(fixture(n)).unwrap();
    let tls = ServerTlsConfig::new()
        .identity(Identity::from_pem(read("server.pem"), read("server.key")))
        .client_ca_root(Certificate::from_pem(read("ca.pem")));
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let (_reporter, health) = tonic_health::server::health_reporter();
            tonic::transport::Server::builder().tls_config(tls).unwrap().add_service(health).serve(addr).await.unwrap();
        });
    });
    for _ in 0..50 {
        if std::net::TcpStream::connect(addr).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    format!("grpcs://localhost:{port}")
}

#[test]
fn mutual_tls_with_client_certificate() {
    use pakpos_core::http::ClientCert;
    let url = start_mtls_server();
    let protos = GrpcConfig::default();
    let health = r#"syntax = "proto3"; package grpc.health.v1;
        message HealthCheckRequest { string service = 1; }
        message HealthCheckResponse { int32 status = 1; }
        service Health { rpc Check(HealthCheckRequest) returns (HealthCheckResponse); }"#;
    let dir = tempfile::tempdir().unwrap();
    let proto = dir.path().join("health.proto");
    std::fs::write(&proto, health).unwrap();
    let cfg = GrpcConfig { proto_files: vec![proto.display().to_string()], ..protos };

    let ca_only =
        ClientCert { enabled: true, host: "localhost".into(), ca_path: fixture("ca.pem"), ..Default::default() };
    let with_cert = ClientCert { cert_path: fixture("client.pem"), key_path: fixture("client.key"), ..ca_only.clone() };

    let settings = HttpSettings { client_certs: vec![ca_only], ..Default::default() };
    let pool = grpc::load_pool(&cfg, &url, &settings).unwrap();
    let denied = grpc::call(&url, &pool, "grpc.health.v1.Health/Check", "{}", &[], &settings, 1);
    assert!(denied.map(|r| !r.is_ok()).unwrap_or(true), "server must reject a client without certificate");

    let settings = HttpSettings { client_certs: vec![with_cert], ..Default::default() };
    let ok = grpc::call(&url, &pool, "grpc.health.v1.Health/Check", "{}", &[], &settings, 1).unwrap();
    assert!(ok.is_ok(), "{ok:?}");
    assert!(ok.messages[0].contains("\"status\": 1"), "{}", ok.messages[0]);
}
