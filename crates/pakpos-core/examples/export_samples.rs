//! Regenerates the files in the repository's `examples/` folder.
//!
//! `cargo run -p pakpos-core --example export_samples -- examples`

use pakpos_core::{postman, samples};
use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "examples".into()));
    std::fs::create_dir_all(&dir)?;
    let mut collection = samples::demo_collection();
    collection.id = "pakpos-demo".into();
    let mut local = samples::demo_environment("http://127.0.0.1:8787");
    local.id = "local".into();
    local.name = "Local mock".into();
    let mut httpbin = samples::demo_environment("https://httpbin.org");
    httpbin.id = "httpbin".into();

    let write = |name: &str, text: String| std::fs::write(dir.join(name), text + "\n");
    write("pakpos-demo.collection.json", serde_json::to_string_pretty(&collection).unwrap())?;
    write("pakpos-demo.postman_collection.json", postman::export_collection(&collection))?;
    write("local.environment.json", serde_json::to_string_pretty(&local).unwrap())?;
    write("httpbin.environment.json", serde_json::to_string_pretty(&httpbin).unwrap())?;
    write("httpbin.postman_environment.json", postman::export_environment(&httpbin))?;
    println!("samples written to {}", dir.display());
    Ok(())
}
