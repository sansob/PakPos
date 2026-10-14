//! Sample collection and environment used on first launch and in the docs.

use crate::assertions::Operator;
use crate::model::{
    Assertion, Auth, Body, Collection, Environment, Extraction, Folder, HttpMethod, Item, KeyValue, RawLanguage,
    Request,
};

fn test(target: &str, op: Operator, expected: &str) -> Assertion {
    Assertion { enabled: true, target: target.into(), op, expected: expected.into() }
}

fn req(name: &str, method: HttpMethod, url: &str) -> Request {
    Request { name: name.into(), method, url: url.into(), ..Default::default() }
}

/// A demo collection that targets an httpbin-compatible API at `{{base_url}}`.
pub fn demo_collection() -> Collection {
    let mut get = req("Get with query params", HttpMethod::Get, "{{base_url}}/get");
    get.params = vec![KeyValue::new("lang", "id"), KeyValue::new("app", "pakpos")];
    get.headers = vec![KeyValue::new("Accept", "application/json")];
    get.assertions = vec![
        test("status", Operator::Equals, "200"),
        test("json.args.lang", Operator::Equals, "id"),
        test("time", Operator::LessThan, "3000"),
    ];
    get.description = "Sends query parameters and checks they are echoed back.".into();

    let mut post = req("Create user (JSON)", HttpMethod::Post, "{{base_url}}/post");
    post.body = Body::Raw {
        content: "{\n  \"name\": \"Budi Santoso\",\n  \"city\": \"Jakarta\",\n  \"request_id\": \"{{$guid}}\"\n}"
            .into(),
        language: RawLanguage::Json,
    };
    post.assertions = vec![
        test("status", Operator::Equals, "200"),
        test("json.json.city", Operator::Equals, "Jakarta"),
        test("header.Content-Type", Operator::Contains, "json"),
    ];

    let example = |name: &str, status: u16, body: &str| crate::model::Example {
        id: crate::model::new_id(),
        name: name.into(),
        status,
        headers: vec![KeyValue::new("Content-Type", "application/json")],
        body: body.into(),
    };
    post.examples = vec![
        example(
            "Created",
            201,
            "{\n  \"id\": \"usr_1001\",\n  \"name\": \"Budi Santoso\",\n  \"city\": \"Jakarta\"\n}",
        ),
        example("Validation error", 422, "{\n  \"error\": \"name is required\"\n}"),
    ];

    let mut form = req("Submit form", HttpMethod::Post, "{{base_url}}/post");
    form.body =
        Body::FormUrlEncoded { fields: vec![KeyValue::new("username", "budi"), KeyValue::new("remember", "true")] };
    form.assertions = vec![test("json.form.username", Operator::Equals, "budi")];

    let mut put = req("Update user", HttpMethod::Put, "{{base_url}}/put");
    put.body = Body::Raw { content: "{ \"city\": \"Bandung\" }".into(), language: RawLanguage::Json };
    let mut patch = req("Patch user", HttpMethod::Patch, "{{base_url}}/patch");
    patch.body = Body::Raw { content: "{ \"active\": true }".into(), language: RawLanguage::Json };
    let mut del = req("Delete user", HttpMethod::Delete, "{{base_url}}/delete");
    del.assertions = vec![test("status", Operator::Equals, "200")];

    let mut bearer = req("Bearer token", HttpMethod::Get, "{{base_url}}/bearer");
    bearer.auth = Auth::Bearer { token: "{{token}}".into() };
    bearer.assertions = vec![test("json.authenticated", Operator::Equals, "true")];
    let mut basic = req("Basic auth", HttpMethod::Get, "{{base_url}}/basic-auth/pakpos/rahasia");
    basic.auth = Auth::Basic { username: "pakpos".into(), password: "rahasia".into() };
    basic.assertions = vec![test("status", Operator::Equals, "200")];
    let mut cookies = req("Set a cookie", HttpMethod::Get, "{{base_url}}/cookies/set?flavour=kopi-susu");
    cookies.assertions = vec![test("json.cookies.flavour", Operator::Equals, "kopi-susu")];
    let mut digest = req("Digest auth", HttpMethod::Get, "{{base_url}}/digest-auth/auth/pakpos/rahasia");
    digest.auth = Auth::Digest { username: "pakpos".into(), password: "rahasia".into() };
    digest.assertions = vec![test("json.authenticated", Operator::Equals, "true")];

    let mut uuid = req("Get UUID (extract)", HttpMethod::Get, "{{base_url}}/uuid");
    uuid.extractions = vec![Extraction { enabled: true, variable: "last_uuid".into(), source: "json.uuid".into() }];
    uuid.assertions = vec![test("json.uuid", Operator::Exists, "")];
    uuid.examples = vec![crate::model::Example {
        id: crate::model::new_id(),
        name: "Random UUID".into(),
        status: 200,
        headers: vec![KeyValue::new("Content-Type", "application/json")],
        body: "{ \"uuid\": \"0f8e6a7c-2b1d-4c3e-9f5a-8d7b6c5e4f3a\" }".into(),
    }];
    let mut json = req("JSON document", HttpMethod::Get, "{{base_url}}/json");
    json.assertions = vec![test("json.slideshow.author", Operator::Exists, "")];
    let mut not_found = req("Expect 404", HttpMethod::Get, "{{base_url}}/status/404");
    not_found.assertions = vec![test("status", Operator::Equals, "404")];

    let mut oauth = req("Profile (OAuth 2.0)", HttpMethod::Get, "{{base_url}}/oauth/me");
    oauth.auth = Auth::OAuth2(crate::model::OAuth2Config {
        grant: crate::model::OAuth2Grant::ClientCredentials,
        token_url: "{{base_url}}/oauth/token".into(),
        auth_url: "{{base_url}}/oauth/authorize".into(),
        client_id: "pakpos-demo".into(),
        client_secret: "demo-secret".into(),
        scope: "read".into(),
        ..Default::default()
    });
    oauth.assertions = vec![test("status", Operator::Equals, "200"), test("json.user", Operator::Equals, "budi")];
    oauth.description = "The token is fetched automatically (client credentials). Switch the grant to Authorization code to try the browser login against the mock API.".into();

    let mut gql = req("Users by city (GraphQL)", HttpMethod::Post, "{{base_url}}/graphql");
    gql.body = Body::GraphQl {
        query: "query Users($city: String) {\n  users(city: $city) {\n    id\n    name\n    city\n  }\n}\n".into(),
        variables: "{ \"city\": \"Jakarta\" }".into(),
        operation_name: String::new(),
    };
    gql.assertions = vec![
        test("json.errors", Operator::NotExists, ""),
        test("json.data.users[0].city", Operator::Equals, "Jakarta"),
    ];
    gql.description = "Click “Fetch schema” in the Body tab to browse the API.".into();

    let mut ws = req("Live prices (WebSocket)", HttpMethod::Get, "{{ws_url}}/ws");
    ws.kind = crate::model::RequestKind::WebSocket;
    ws.auth = Auth::Bearer { token: "{{token}}".into() };
    ws.body = Body::Raw {
        content: "{\n  \"type\": \"subscribe\",\n  \"symbol\": \"IDR\"\n}".into(),
        language: RawLanguage::Json,
    };
    ws.description = "Works with the offline mock API (`python3 scripts/mock_httpbin.py`, environment “Local mock”): connect, then send the message from the Message tab. The server echoes it and pushes a tick every 2 seconds.".into();

    let mut scripted = req("Scripted checks (JavaScript)", HttpMethod::Get, "{{base_url}}/json");
    scripted.scripts = crate::model::Scripts {
        pre_request: "// Runs before sending: add a trace header.\npm.request.headers.upsert({ key: \"X-Trace-Id\", value: pm.variables.replaceIn(\"{{$guid}}\") });\n".into(),
        test: "pm.test(\"Status code is 200\", () => {\n  pm.response.to.have.status(200);\n});\n\npm.test(\"Slideshow has slides\", () => {\n  const json = pm.response.json();\n  pm.expect(json.slideshow.slides).to.be.an(\"array\").that.is.not.empty;\n  pm.expect(json.slideshow.author).to.eql(\"PakPos\");\n});\n\npm.environment.set(\"slide_count\", pm.response.json().slideshow.slides.length);\nconsole.log(\"slides:\", pm.environment.get(\"slide_count\"));\n".into(),
    };
    scripted.description = "Postman-style pm.* scripts, run in a sandbox. Collections you import must be trusted before their scripts run.".into();

    let mut grpc_req = req("Health check (gRPC)", HttpMethod::Post, "{{grpc_url}}");
    grpc_req.kind = crate::model::RequestKind::Grpc;
    grpc_req.grpc =
        Some(crate::model::GrpcConfig { method: "grpc.health.v1.Health/Check".into(), ..Default::default() });
    grpc_req.body = Body::Raw { content: "{\n  \"service\": \"pakpos.demo\"\n}".into(), language: RawLanguage::Json };
    grpc_req.assertions = vec![test("status", Operator::Equals, "0"), test("json.status", Operator::Exists, "")];
    grpc_req.description = "Start the demo server with `cargo run -p pakpos-core --example grpc_demo_server`, then Load methods (server reflection) and Send.".into();

    let mut basics = Folder::new("Basics");
    basics.items = [get, post, form, put, patch, del, cookies].into_iter().map(Item::Request).collect();
    let mut auth = Folder::new("Auth");
    auth.items = vec![Item::Request(bearer), Item::Request(basic), Item::Request(digest), Item::Request(oauth)];
    let mut responses = Folder::new("Responses");
    responses.items = vec![Item::Request(uuid), Item::Request(json), Item::Request(not_found), Item::Request(scripted)];
    let mut graphql = Folder::new("GraphQL");
    graphql.items = vec![Item::Request(gql)];
    let mut realtime = Folder::new("Realtime");
    let mut chat = req("Chat (gRPC stream)", HttpMethod::Post, "{{grpc_url}}");
    chat.kind = crate::model::RequestKind::Grpc;
    chat.grpc = Some(crate::model::GrpcConfig { method: "pakpos.demo.Chat/Talk".into(), ..Default::default() });
    chat.body = Body::Raw {
        content: "{\n  \"user\": \"budi\",\n  \"text\": \"paket saya sudah sampai mana?\"\n}".into(),
        language: RawLanguage::Json,
    };
    chat.description =
        "A bidirectional stream: Open stream, then Send message as often as you like; End stream when done.".into();
    let mut sse = req("Price stream (SSE)", HttpMethod::Get, "{{base_url}}/sse");
    sse.kind = crate::model::RequestKind::Sse;
    let mut sio = req("Parcel events (Socket.IO)", HttpMethod::Get, "{{base_url}}");
    sio.kind = crate::model::RequestKind::SocketIo;
    sio.realtime = Some(crate::model::RealtimeConfig { event: "track".into(), ..Default::default() });
    sio.body = Body::Raw { content: "{ \"resi\": \"PP-2026-0042\" }".into(), language: RawLanguage::Json };
    let mut mqtt = req("Office sensors (MQTT)", HttpMethod::Get, "{{mqtt_url}}");
    mqtt.kind = crate::model::RequestKind::Mqtt;
    mqtt.realtime = Some(crate::model::RealtimeConfig {
        subscriptions: vec![crate::model::MqttTopic { enabled: true, topic: "kantor/#".into(), qos: 1 }],
        topic: "kantor/1/lampu".into(),
        ..Default::default()
    });
    mqtt.body = Body::Raw { content: "{ \"on\": true }".into(), language: RawLanguage::Json };
    mqtt.description = "Start the demo broker with `python3 scripts/mock_mqtt.py`, then Connect.".into();
    realtime.items = vec![
        Item::Request(ws),
        Item::Request(sse),
        Item::Request(sio),
        Item::Request(mqtt),
        Item::Request(grpc_req),
        Item::Request(chat),
    ];

    let mut c = Collection::new("PakPos Demo");
    c.description =
        "Sample requests against an httpbin-compatible API. Select the “Demo (httpbin)” environment and press Send."
            .into();
    c.items = vec![
        Item::Folder(basics),
        Item::Folder(auth),
        Item::Folder(graphql),
        Item::Folder(realtime),
        Item::Folder(responses),
    ];
    c
}

/// Environment for [`demo_collection`].
pub fn demo_environment(base_url: &str) -> Environment {
    let mut env = Environment::new("Demo (httpbin)");
    env.set("base_url", base_url);
    env.set("token", "pakpos-demo-token");
    env.set("ws_url", crate::ws::to_ws_url(base_url));
    env.set("grpc_url", "grpc://127.0.0.1:50051");
    env.set("mqtt_url", "mqtt://127.0.0.1:1883");
    env
}

#[cfg(test)]
mod tests {
    #[test]
    fn demo_has_requests() {
        let c = super::demo_collection();
        assert_eq!(c.request_count(), 22);
        assert_eq!(super::demo_environment("x").get("base_url"), Some("x"));
    }
}
