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

    let mut uuid = req("Get UUID (extract)", HttpMethod::Get, "{{base_url}}/uuid");
    uuid.extractions = vec![Extraction { enabled: true, variable: "last_uuid".into(), source: "json.uuid".into() }];
    uuid.assertions = vec![test("json.uuid", Operator::Exists, "")];
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

    let mut basics = Folder::new("Basics");
    basics.items = [get, post, form, put, patch, del].into_iter().map(Item::Request).collect();
    let mut auth = Folder::new("Auth");
    auth.items = vec![Item::Request(bearer), Item::Request(basic), Item::Request(oauth)];
    let mut responses = Folder::new("Responses");
    responses.items = vec![Item::Request(uuid), Item::Request(json), Item::Request(not_found)];

    let mut c = Collection::new("PakPos Demo");
    c.description =
        "Sample requests against an httpbin-compatible API. Select the “Demo (httpbin)” environment and press Send."
            .into();
    c.items = vec![Item::Folder(basics), Item::Folder(auth), Item::Folder(responses)];
    c
}

/// Environment for [`demo_collection`].
pub fn demo_environment(base_url: &str) -> Environment {
    let mut env = Environment::new("Demo (httpbin)");
    env.set("base_url", base_url);
    env.set("token", "pakpos-demo-token");
    env
}

#[cfg(test)]
mod tests {
    #[test]
    fn demo_has_requests() {
        let c = super::demo_collection();
        assert_eq!(c.request_count(), 12);
        assert_eq!(super::demo_environment("x").get("base_url"), Some("x"));
    }
}
