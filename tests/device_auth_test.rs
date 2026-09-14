//! Device authorization flow: `auth-check` → credential dialog → `auth`
//! → cached credentials used by the proxied device endpoints.
//!
//! The device mock plays an `oc serve` whose Basic Auth differs from the
//! portal's unified credentials (`tester`/`portal-secret` in
//! [`common::TEST_USER`]/[`common::TEST_PASS`]), reproducing the
//! "device auth failed: ... returned 401" scenario from the field.

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// base64("device-user:device-pass") — credentials the device expects,
/// deliberately different from the portal's `tester`/`portal-secret`.
fn device_basic_auth() -> &'static str {
    "Basic ZGV2aWNlLXVzZXI6ZGV2aWNlLXBhc3M="
}

fn projects_body() -> String {
    r#"[{"path":"/Users/samuel/projects/foo","lastOpenedAt":"2026-09-12T10:00:00+08:00"}]"#.into()
}

/// Mount the device mock: `/project` is 200 with the right Basic Auth,
/// 401 otherwise. wiremock checks mocks in mounting order (first mounted
/// wins), so the header-matching mock must be mounted BEFORE the 401
/// catch-all.
async fn mount_device_mock(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/project"))
        .and(wiremock::matchers::header("authorization", device_basic_auth()))
        .respond_with(ResponseTemplate::new(200).set_body_string(projects_body()))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/project"))
        .respond_with(ResponseTemplate::new(401))
        .mount(server)
        .await;
}

fn auth_check_uri(pctype: &str, pcname: &str) -> String {
    format!("/api/devices/{}/{}/auth-check", pctype, pcname)
}

fn auth_uri(pctype: &str, pcname: &str) -> String {
    format!("/api/devices/{}/{}/auth", pctype, pcname)
}

#[tokio::test]
async fn auth_check_reports_required_when_unified_creds_rejected() {
    let sb = MockServer::start().await;
    let device = MockServer::start().await;
    mount_device_mock(&device).await;
    sb_get(&sb, &devices_fs_path(), 200, &one_device_json("macos", "samuel", &device.uri())).await;

    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    // The BFF's unified credentials (tester/portal-secret) are NOT what
    // the device wants → the probe must surface `required: true`.
    let resp = send(&app, authed_get(&auth_check_uri("macos", "samuel"), &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["required"], serde_json::json!(true));
}

#[tokio::test]
async fn auth_check_passes_without_session() {
    let sb = MockServer::start().await;
    sb_get(&sb, &devices_fs_path(), 200, &one_device_json("macos", "samuel", "http://127.0.0.1:1")).await;

    let app = test_app(build_state(&sb.uri()));
    let resp = send(&app, req(Method::GET, &auth_check_uri("macos", "samuel"))).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_rejects_bad_credentials_with_device_auth_failed() {
    let sb = MockServer::start().await;
    let device = MockServer::start().await;
    mount_device_mock(&device).await;
    sb_get(&sb, &devices_fs_path(), 200, &one_device_json("macos", "samuel", &device.uri())).await;

    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    let resp = send(
        &app,
        json_req(
            Method::POST,
            &auth_uri("macos", "samuel"),
            &serde_json::json!({"username": "device-user", "password": "wrong"}),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], serde_json::json!("device_auth_failed"));
}

#[tokio::test]
async fn auth_verifies_then_proxied_calls_use_cached_credentials() {
    let sb = MockServer::start().await;
    let device = MockServer::start().await;
    mount_device_mock(&device).await;
    sb_get(&sb, &devices_fs_path(), 200, &one_device_json("macos", "samuel", &device.uri())).await;

    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    // 1. Before authorizing: the auth-check probe (live device hop) says
    //    credentials are missing/wrong → dialog required.
    let resp = send(&app, authed_get(&auth_check_uri("macos", "samuel"), &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await["required"], serde_json::json!(true));

    // 2. User authorizes with the device's real credentials.
    let resp = send(
        &app,
        json_req(
            Method::POST,
            &auth_uri("macos", "samuel"),
            &serde_json::json!({"username": "device-user", "password": "device-pass"}),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK, "valid credentials must be accepted");
    let body = body_json(resp).await;
    assert_eq!(body["ok"], serde_json::json!(true));

    // 3. The cached pair now satisfies the live device probe.
    let resp = send(&app, authed_get(&auth_check_uri("macos", "samuel"), &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["required"], serde_json::json!(false));
}

#[tokio::test]
async fn jump_url_embeds_cached_credentials_as_auth_token() {
    let sb = MockServer::start().await;
    let device = MockServer::start().await;
    mount_device_mock(&device).await;
    sb_get(&sb, &devices_fs_path(), 200, &one_device_json("macos", "samuel", &device.uri())).await;

    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    // Authorize with the device's real credentials first.
    let resp = send(
        &app,
        json_req(
            Method::POST,
            &auth_uri("macos", "samuel"),
            &serde_json::json!({"username": "device-user", "password": "device-pass"}),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // GET /jump?format=json must return a URL carrying the cached pair
    // as opencode's auth_token query param (standard base64, encoded).
    let uri = "/api/devices/macos/samuel/jump?directory=%2F&session=ses_abc&format=json";
    let resp = send(&app, authed_get(uri, &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let jump = body["jump_url"].as_str().expect("jump_url in response");

    use base64::Engine;
    let token = base64::engine::general_purpose::STANDARD.encode("device-user:device-pass");
    let expected = format!("auth_token={}", urlencoding::encode(&token));
    assert!(jump.contains(&expected), "jump={} should contain {}", jump, expected);
    assert!(jump.starts_with(&device.uri()), "jump={} should target the device", jump);
    assert!(jump.contains("/session/ses_abc"));
}

