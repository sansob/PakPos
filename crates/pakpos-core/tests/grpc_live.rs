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
