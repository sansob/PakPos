//! The MCP client talking to PakPos's own MCP server over stdio.

use pakpos_core::http::HttpSettings;
use pakpos_core::mcp_client::McpClient;
use serde_json::json;

#[test]
fn stdio_client_against_pakpos_mcp() {
    let home = tempfile::tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_pakpos-cli");
    let target = format!("\"{bin}\" mcp");
    let env = vec![("PAKPOS_HOME".to_string(), home.path().display().to_string())];
    let mut c = McpClient::connect(&target, &[], &env, &HttpSettings::default()).unwrap();
    assert_eq!(c.server.name, "pakpos");
    let tools: Vec<String> =
        c.list("tools").unwrap().iter().map(|t| t["name"].as_str().unwrap_or("").to_string()).collect();
    assert!(tools.contains(&"list_collections".to_string()), "{tools:?}");
    let r = c.call_tool("list_collections", json!({})).unwrap();
    assert!(r["content"][0]["text"].is_string(), "{r}");
    let missing = c.call_tool("list_requests", json!({"collection": "nope"})).unwrap();
    assert_eq!(missing["isError"], true);
}
