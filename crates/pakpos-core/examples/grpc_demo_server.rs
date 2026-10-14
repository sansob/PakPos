//! Tiny gRPC server for trying PakPos offline: the standard health service
//! plus server reflection.
//!
//! `cargo run -p pakpos-core --example grpc_demo_server -- [port]` (default 50051)

fn main() {
    let port: u16 = std::env::args().nth(1).and_then(|p| p.parse().ok()).unwrap_or(50051);
    let addr: std::net::SocketAddr = ([127, 0, 0, 1], port).into();
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime");
    rt.block_on(async move {
        let (reporter, health) = tonic_health::server::health_reporter();
        // A named service that flips between SERVING and NOT_SERVING, so Watch streams something.
        tokio::spawn(async move {
            let mut up = true;
            loop {
                let status =
                    if up { tonic_health::ServingStatus::Serving } else { tonic_health::ServingStatus::NotServing };
                reporter.set_service_status("pakpos.demo", status).await;
                up = !up;
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        });
        let reflection = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(tonic_health::pb::FILE_DESCRIPTOR_SET)
            .build_v1()
            .expect("reflection");
        println!("gRPC demo server on grpc://{addr} (grpc.health.v1.Health + reflection)");
        tonic::transport::Server::builder()
            .add_service(health)
            .add_service(reflection)
            .serve(addr)
            .await
            .expect("serve");
    });
}
