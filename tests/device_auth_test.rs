//! Device-hop credentials derive from the signed-in portal user: the BFF
//! authenticates every device call (and builds the `?auth_token=` deep
//! link) with `{user id}:{login key}` as Basic Auth. The device mock
//! plays an `oc serve` whose `OPENCODE_SERVER_USERNAME/PASSWORD` equal
//! the binding user's id/key — the pairing mini-oc-gui configures via
//! `POST /api/user/info`.

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Basic header value the device expects: base64("{TEST_USER_ID}:{TEST_KEY}").
fn user_basic_auth() -> String {
    use base64::Engine;
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", TEST_USER_ID, TEST_KEY))
    )
}

/// `?auth_token=` query value the BFF must embed for the same pair
/// (standard base64, percent-encoded — mirrors `jump::append_auth_token`).
fn user_auth_token_query() -> String {
    use base64::Engine;
    let token =
        base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", TEST_USER_ID, TEST_KEY));
    urlencoding::encode(&token).to_string()
}

async fn mount_registry_with_device_url(sb: &MockServer, public_url: &str) {
    sb_get(
        sb,
        users_fs_path(),
        200,
        &users_json_with_public_urls("127.0.0.1", &[("samuel", public_url)]),
    )
    .await;
}

#[tokio::test]
async fn device_calls_authenticate_with_user_id_and_key() {
    let sb = MockServer::start().await;
    let device = MockServer::start().await;

    // `POST /api/session` accepts only the user-derived Basic pair.
    Mock::given(method("POST"))
        .and(path("/api/session"))
        .and(wiremock::matchers::header("authorization", user_basic_auth()))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"id": "sess-9", "directory": "/Users/samuel/projects/foo"}),
        ))
        .expect(1)
        .mount(&device)
        .await;
    mount_registry_with_device_url(&sb, &device.uri()).await;

    let app = test_app(build_state(&sb.uri()));
    let cookie = login_mounted(&app).await;

    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/devices/macos/samuel/sessions",
            &serde_json::json!({"directory": "/Users/samuel/projects/foo"}),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["id"], "sess-9");
    device.verify().await;
}

#[tokio::test]
async fn rejected_user_credentials_surface_device_auth_failed() {
    let sb = MockServer::start().await;
    let device = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/session"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&device)
        .await;
    mount_registry_with_device_url(&sb, &device.uri()).await;

    let app = test_app(build_state(&sb.uri()));
    let cookie = login_mounted(&app).await;

    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/devices/macos/samuel/sessions",
            &serde_json::json!({"directory": "/Users/samuel/projects/foo"}),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], serde_json::json!("device_auth_failed"));
}

#[tokio::test]
async fn jump_url_embeds_user_credentials_as_auth_token() {
    let sb = MockServer::start().await;
    let device = MockServer::start().await;
    mount_registry_with_device_url(&sb, &device.uri()).await;

    let app = test_app(build_state(&sb.uri()));
    let cookie = login_mounted(&app).await;

    let uri = "/api/devices/macos/samuel/jump?directory=%2F&session=ses_abc&format=json";
    let resp = send(&app, authed_get(uri, &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let jump = body["jump_url"].as_str().expect("jump_url in response");

    let expected = format!("auth_token={}", user_auth_token_query());
    assert!(jump.contains(&expected), "jump={} should contain {}", jump, expected);
    let base = format!("{}/{}", host_of(&device.uri()), port_of(&device.uri()));
    assert!(jump.starts_with(&base), "jump={} should target the device", jump);
    assert!(jump.contains("/session/ses_abc"));
}

#[tokio::test]
async fn jump_requires_session() {
    let sb = MockServer::start().await;
    mount_registry_with_device_url(&sb, "http://127.0.0.1:1").await;

    let app = test_app(build_state(&sb.uri()));
    let resp = send(
        &app,
        req(
            Method::GET,
            "/api/devices/macos/samuel/jump?directory=%2F&session=ses_abc&format=json",
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
