use super::*;
use crate::http::Response;
use crate::model::HttpMethod;

fn resp(status: u16, body: &str) -> Response {
    Response {
        status,
        status_text: "OK".into(),
        headers: vec![("Content-Type".into(), "application/json".into()), ("X-Id".into(), "7".into())],
        body: body.as_bytes().to_vec(),
        elapsed_ms: 120,
        url: "http://x".into(),
        http_version: "HTTP/1.1".into(),
    }
}

fn req() -> ScriptRequest {
    let mut r = Request::new(HttpMethod::Post, "http://api.test/items");
    r.params.push(KeyValue::new("a", "1"));
    r.headers.push(KeyValue::new("Accept", "application/json"));
    r.body = Body::Raw { content: "{\"x\":1}".into(), language: crate::model::RawLanguage::Json };
    script_request(&r)
}

fn test_script(src: &str, body: &str) -> ScriptOutcome {
    let mut scope = VarScope::new();
    scope.environment.insert("base".into(), "http://api.test".into());
    run(src, Phase::Test, "r", &req(), Some(&resp(200, body)), &scope, Limits::default())
}

#[test]
fn pm_tests_and_expect() {
    let out = test_script(
        r#"
        pm.test("status 200", () => pm.response.to.have.status(200));
        pm.test("json", () => {
            const j = pm.response.json();
            pm.expect(j.items).to.be.an("array").with.lengthOf(2);
            pm.expect(j.items[0]).to.have.property("name", "Budi");
            pm.expect(j.total).to.be.above(1).and.below(10);
            pm.expect(j).to.deep.equal({ items: [{ name: "Budi" }, { name: "Siti" }], total: 2 });
            pm.expect("hello world").to.include("world");
            pm.expect(j.missing).to.be.undefined;
            pm.expect(null).to.not.exist;
        });
        pm.test("header", () => pm.expect(pm.response.headers.get("x-id")).to.eql("7"));
        pm.test("fails", () => pm.expect(pm.response.code).to.equal(201));
        pm.test("time", () => pm.expect(pm.response.responseTime).to.be.below(500));
        console.log("hi", { a: 1 });
        "#,
        r#"{"items":[{"name":"Budi"},{"name":"Siti"}],"total":2}"#,
    );
    assert!(out.error.is_none(), "{:?}", out.error);
    let r: Vec<(String, bool)> = out.tests.iter().map(|t| (t.name.clone(), t.passed)).collect();
    assert_eq!(
        r,
        vec![
            ("status 200".into(), true),
            ("json".into(), true),
            ("header".into(), true),
            ("fails".into(), false),
            ("time".into(), true)
        ]
    );
    assert!(out.tests[3].message.contains("expected 200 to equal 201"), "{}", out.tests[3].message);
    assert_eq!(out.logs, vec!["hi {\"a\":1}"]);
}

#[test]
fn variables_set_unset_and_resolution() {
    let out = test_script(
        r#"
        const token = pm.response.json().token;
        pm.environment.set("token", token);
        pm.collectionVariables.set("count", 3);
        pm.globals.unset("old");
        pm.variables.set("local", "x");
        pm.test("lookup", () => {
            pm.expect(pm.environment.get("base")).to.equal("http://api.test");
            pm.expect(pm.variables.get("token")).to.equal("abc");
            pm.expect(pm.environment.replaceIn("{{base}}/me")).to.equal("http://api.test/me");
        });
        "#,
        r#"{"token":"abc"}"#,
    );
    assert!(out.error.is_none(), "{:?}", out.error);
    assert!(out.tests[0].passed, "{}", out.tests[0].message);
    assert_eq!(out.changes.environment, vec![("token".into(), Some("abc".into()))]);
    assert_eq!(out.changes.collection, vec![("count".into(), Some("3".into()))]);
    assert_eq!(out.changes.globals, vec![("old".into(), None)]);
    assert_eq!(out.changes.local, vec![("local".into(), Some("x".into()))]);
}

#[test]
fn pre_request_can_edit_request() {
    let out = run(
        r#"
        pm.request.headers.upsert({ key: "X-Sig", value: "s1" });
        pm.request.headers.remove("accept");
        pm.request.url = pm.request.url + "&b=2";
        pm.request.body.raw = JSON.stringify({ ts: 1 });
        pm.variables.set("nonce", "n1");
        "#,
        Phase::PreRequest,
        "r",
        &req(),
        None,
        &VarScope::new(),
        Limits::default(),
    );
    assert!(out.error.is_none(), "{:?}", out.error);
    let changed = out.request.unwrap();
    let mut r = Request::new(HttpMethod::Post, "http://api.test/items");
    r.body = Body::Raw { content: String::new(), language: crate::model::RawLanguage::Json };
    apply_request(&mut r, &changed);
    assert_eq!(r.url, "http://api.test/items");
    assert_eq!(r.params, vec![KeyValue::new("a", "1"), KeyValue::new("b", "2")]);
    assert_eq!(r.headers, vec![KeyValue::new("X-Sig", "s1")]);
    assert_eq!(r.body, Body::Raw { content: "{\"ts\":1}".into(), language: crate::model::RawLanguage::Json });
}

#[test]
fn errors_timeouts_and_sandbox() {
    let out = test_script("pm.test('ok', () => {}); throw new Error('boom');", "{}");
    assert_eq!(out.tests.len(), 1, "tests before the error still count");
    assert!(out.error.as_deref().unwrap().contains("boom"));

    let out = test_script("this is not js", "{}");
    assert!(out.error.is_some());

    let limits = Limits { timeout: Duration::from_millis(200), ..Default::default() };
    let started = Instant::now();
    let out = run("while (true) {}", Phase::Test, "r", &req(), None, &VarScope::new(), limits);
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(out.error.as_deref().unwrap().contains("timed out"), "{:?}", out.error);

    // No host access: none of these exist in the sandbox.
    let out = test_script(
        r#"pm.test("sandbox", () => {
            pm.expect(typeof require).to.equal("undefined");
            pm.expect(typeof std).to.equal("undefined");
            pm.expect(typeof os).to.equal("undefined");
            pm.expect(typeof fetch).to.equal("undefined");
        });
        pm.test("sendRequest blocked", () => pm.sendRequest("http://x"));"#,
        "{}",
    );
    assert!(out.tests[0].passed, "{}", out.tests[0].message);
    assert!(!out.tests[1].passed);

    let limits = Limits { memory_bytes: 8 * 1024 * 1024, ..Default::default() };
    let out = run(
        "let a = []; while (true) a.push('x'.repeat(1000));",
        Phase::Test,
        "r",
        &req(),
        None,
        &VarScope::new(),
        limits,
    );
    assert!(out.error.is_some());
}

#[test]
fn legacy_tests_object() {
    let out = test_script(
        r#"tests["Status code is 200"] = responseCode.code === 200; tests["body"] = responseBody.includes("nope");"#,
        "{}",
    );
    assert_eq!(out.tests.len(), 2);
    assert!(out.tests[0].passed);
    assert!(!out.tests[1].passed);
}
