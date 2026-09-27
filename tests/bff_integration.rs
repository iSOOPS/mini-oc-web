//! Full-chain BFF integration tests.
//!
//! Each test hand-builds an `AppState` (see `common::build_state`) around
//! fresh wiremock stand-ins for SilverBullet and the per-device `oc serve`
//! instances, mounts the real `build_router` router, and drives it with
//! `tower::ServiceExt::oneshot` — no real ports, no env vars, no live SB
//! login. All tests run serially (`serial_test::serial`) so shared ambient
//! state can never bleed across cases.

mod common;

use axum::http::{header, Method, StatusCode};
use common::*;
use serial_test::serial;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---------------------------------------------------------------------------
// 1. healthz
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn healthz_returns_ok() {
    let sb = MockServer::start().await;
    let app = test_app(build_state(&sb.uri()));

    let resp = send(&app, req(Method::GET, "/healthz")).await;

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_string(resp).await, "ok\n");
}

// ---------------------------------------------------------------------------
// 2-4. login + rate limiting
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn login_success_sets_cookie() {
    let sb = MockServer::start().await;
    sb_get(&sb, users_fs_path(), 200, &users_json(DEFAULT_DEVICES)).await;
    let app = test_app(build_state(&sb.uri()));

    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/login",
            &serde_json::json!({"key": TEST_KEY}),
            None,
        ),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let set_cookie = resp
        .headers()
        .get(header::SET_COOKIE)
        .expect("successful login must set a session cookie")
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        set_cookie.starts_with(&format!("{COOKIE_NAME}=")),
        "cookie should be named {COOKIE_NAME}, got: {set_cookie}"
    );
    assert!(
        set_cookie.contains("HttpOnly"),
        "session cookie must be HttpOnly, got: {set_cookie}"
    );
    assert_eq!(body_json(resp).await, serde_json::json!({"ok": true}));

    // /api/me answers with the user profile (no key material).
    let cookie = set_cookie.split(';').next().unwrap().trim().to_string();
    let resp = send(&app, authed_get("/api/me", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["name"], TEST_USER);
    assert!(
        body.get("oc_serve_port").is_none(),
        "oc_serve_port is gone from /api/me"
    );
    assert!(
        body["devices"].as_array().is_some_and(|a| !a.is_empty()),
        "devices list (设备清单) is structured entries"
    );
    assert_eq!(body["devices"][0]["port"], 4040);
    assert!(body.get("key").is_none(), "me must not leak the key");
}

#[tokio::test]
#[serial]
async fn login_unknown_key_returns_401() {
    let sb = MockServer::start().await;
    // No users document mounted → empty registry → any key is unknown.
    let app = test_app(build_state(&sb.uri()));

    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/login",
            &serde_json::json!({"key": "0123456789abcdef0123456789abcdef"}),
            None,
        ),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert!(
        resp.headers().get(header::SET_COOKIE).is_none(),
        "failed login must not set a session cookie"
    );
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "unauthorized");
}

#[tokio::test]
#[serial]
async fn login_rate_limited_after_5_failures() {
    let sb = MockServer::start().await;
    let app = test_app(build_state(&sb.uri()));

    // Five bad keys → five recorded failures (each answered 401).
    for _ in 0..5 {
        let resp = send(
            &app,
            json_req(
                Method::POST,
                "/api/login",
                &serde_json::json!({"key": "nope-nope-nope-nope-nope-nope32"}),
                None,
            ),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // The 6th attempt is blocked before the key is even checked — even
    // with a valid key.
    sb_get(&sb, users_fs_path(), 200, &users_json(DEFAULT_DEVICES)).await;
    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/login",
            &serde_json::json!({"key": TEST_KEY}),
            None,
        ),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "rate_limited");
}

// ---------------------------------------------------------------------------
// 5-7. /api/me (the only "list my devices" endpoint after the C plan)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn me_requires_session() {
    let sb = MockServer::start().await;
    let app = test_app(build_state(&sb.uri()));

    let resp = send(&app, req(Method::GET, "/api/me")).await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "unauthorized");
}

#[tokio::test]
#[serial]
async fn me_devices_empty_when_user_has_no_devices() {
    // After the C plan removed devices.json, `/api/me.devices` reflects
    // the user registry's `devices` list directly. A user with an empty
    // list must surface an empty array (NOT a 503 from a missing
    // devices.json — that path no longer exists).
    let sb = MockServer::start().await;
    sb_get(
        &sb,
        users_fs_path(),
        200,
        r#"{"version":1,"users":[{"id":"100001","name":"tester","key":"k1234567890123456789012345678901","cloud_ip":"127.0.0.1","devices":[],"created_at":"2026-09-13T00:00:00+08:00","updated_at":"2026-09-13T00:00:00+08:00"}]}"#,
    )
    .await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = login_mounted(&app).await;
    let resp = send(&app, authed_get("/api/me", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(body["devices"].is_array(), "devices field must be an array");
    assert_eq!(body["devices"].as_array().unwrap().len(), 0);
}

// ---------------------------------------------------------------------------
// 10-12. device proxying (projects / sessions / create-session)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn device_projects_returns_proxy_data() {
    let sb = MockServer::start().await;

    // Projects come from the SB-side path-list.md registry (maintained by
    // the mini-oc-gui end-side), not the device's live API. Multi-tenant
    // location: serv/opencode/{user_id}/{pctype}/{pcname}/path-list.md —
    // the third segment is the device SERVICE NAME (registry entry's
    // `name`, same as the URL pcname), NOT the client-reported
    // device-name. Sections are objects (live wire shape, verified
    // against the real SB document).
    sb_get(
        &sb,
        "serv/opencode/100001/windows/dev-one/path-list.md",
        200,
        r#"[
            {"path":"D:/work/api","sections":[{"id":"s1","title":"api session","directory":"D:/work/api/","createdAt":"2026-09-01T10:00:00+08:00","updatedAt":"2026-09-02T09:00:00+08:00"}],"createdAt":"2026-09-01T10:00:00+08:00","lastOpenedAt":"2026-09-02T10:00:00+08:00"},
            {"path":"D:/work/gui","sections":[]}
        ]"#,
    )
    .await;

    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    let resp = send(
        &app,
        authed_get("/api/devices/macos/dev-one/projects", &cookie),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body.as_array().unwrap().len(), 2);
    assert_eq!(body[0]["path"], "D:/work/api");
    assert_eq!(body[0]["lastOpenedAt"], "2026-09-02T10:00:00+08:00");
    assert_eq!(body[1]["path"], "D:/work/gui");
}

#[tokio::test]
#[serial]
async fn device_projects_missing_path_list_returns_empty() {
    let sb = MockServer::start().await;
    sb_get_404(&sb, "serv/opencode/100001/windows/dev-one/path-list.md").await;

    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    let resp = send(
        &app,
        authed_get("/api/devices/macos/dev-one/projects", &cookie),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await, serde_json::json!([]));
}

#[tokio::test]
#[serial]
async fn device_sessions_returns_proxy_data() {
    let sb = MockServer::start().await;

    sb_get(
        &sb,
        "serv/opencode/100001/windows/dev-one/path-list.md",
        200,
        r#"[
            {"path":"D:/work/api","sections":[{"id":"s1","title":"one","createdAt":"2026-09-01T10:00:00+08:00","updatedAt":"2026-09-03T10:00:00+08:00"},{"id":"s2","createdAt":"2026-09-01T10:00:00+08:00"}],"createdAt":"2026-09-01T10:00:00+08:00","lastOpenedAt":"2026-09-03T11:00:00+08:00"},
            {"path":"D:/work/other","sections":[{"id":"s9","title":"x","createdAt":"2026-09-01T10:00:00+08:00"}]}
        ]"#,
    )
    .await;

    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    let resp = send(
        &app,
        authed_get(
            "/api/devices/macos/dev-one/sessions?directory=D%3A%2Fwork%2Fapi",
            &cookie,
        ),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body.as_array().unwrap().len(), 2);
    assert_eq!(body[0]["id"], "s1");
    assert_eq!(body[1]["id"], "s2");
    // Section-level title and updatedAt map through when present.
    assert_eq!(body[0]["title"], "one");
    assert_eq!(body[0]["updatedAt"], "2026-09-03T10:00:00+08:00");
    // Sections without updatedAt fall back to the entry's lastOpenedAt.
    assert_eq!(body[1]["updatedAt"], "2026-09-03T11:00:00+08:00");
}

#[tokio::test]
#[serial]
async fn device_create_session_returns_jump_url() {
    let sb = MockServer::start().await;
    let device = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"id": "sess-42", "directory": "/work/api"}),
        ))
        .expect(1)
        .mount(&device)
        .await;
    sb_get(
        &sb,
        users_fs_path(),
        200,
        &users_json_with_public_urls("127.0.0.1", &[("dev-one", &device.uri())]),
    )
    .await;

    let app = test_app(build_state(&sb.uri()));
    let cookie = login_mounted(&app).await;

    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/devices/macos/dev-one/sessions",
            &serde_json::json!({"directory": "/work/api", "title": "demo"}),
            Some(&cookie),
        ),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["id"], "sess-42");
    assert_eq!(body["directory"], "/work/api");
    let expected_jump = format!(
        "{}/{}/session/sess-42?auth_token={}",
        device.uri(),
        b64url("/work/api"),
        auth_token_query(TEST_USER_ID, TEST_KEY)
    );
    assert_eq!(body["jump_url"], expected_jump);
}

/// `?auth_token=` value the BFF embeds for the signed-in user's own
/// account credentials — standard base64 of `{user id}:{login key}`,
/// percent-encoded (see `jump::append_auth_token`).
fn auth_token_query(user: &str, pass: &str) -> String {
    use base64::Engine;
    let token = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", user, pass));
    urlencoding::encode(&token).to_string()
}

// ---------------------------------------------------------------------------
// 16. device detail
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn device_detail_reports_identity_and_user_credentials() {
    let sb = MockServer::start().await;
    sb_get(
        &sb,
        users_fs_path(),
        200,
        &users_json_with_public_urls(
            "127.0.0.1",
            &[("demo-pc", "http://127.0.0.1:9464")],
        ),
    )
    .await;

    let app = test_app(build_state(&sb.uri()));
    let cookie = login_mounted(&app).await;

    let resp = send(
        &app,
        authed_get("/api/devices/windows/demo-pc/detail", &cookie),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["pctype"], "windows");
    assert_eq!(body["pcname"], "demo-pc");
    assert_eq!(body["public_url"], "http://127.0.0.1");
    assert_eq!(body["username"], TEST_USER_ID);
    assert_eq!(body["password"], TEST_KEY);
}

#[tokio::test]
#[serial]
async fn device_detail_requires_session() {
    let sb = MockServer::start().await;
    let app = test_app(build_state(&sb.uri()));

    let resp = send(&app, req(Method::GET, "/api/devices/windows/demo-pc/detail")).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// me returns enriched devices (NEW — PR 2026-09-14 batch API)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn me_returns_enriched_devices_with_runtime_status() {
    let sb = MockServer::start().await;
    let online_dev = MockServer::start().await;
    let offline_dev = MockServer::start().await; // no mocks mounted → probe fails

    Mock::given(method("GET"))
        .and(path("/status"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"opencode_serve":"running","rathole":"running","system":{"hostname":"box1"},"version":"0.5"}"#,
        ))
        .mount(&online_dev)
        .await;

    // Each device's public-url lives on its own UserDevice entry; the
    // BFF probes `<public-url>/status` per entry.
    sb_get(
        &sb,
        users_fs_path(),
        200,
        &users_json_with_public_urls(
            "127.0.0.1",
            &[
                ("dev-online", online_dev.uri().as_str()),
                ("dev-offline", offline_dev.uri().as_str()),
            ],
        ),
    )
    .await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = login_mounted(&app).await;

    let resp = send(&app, authed_get("/api/me", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let devices = body["devices"].as_array().expect("devices[]");
    assert_eq!(devices.len(), 2);

    let online = devices
        .iter()
        .find(|d| d["name"] == "dev-online")
        .unwrap();
    assert_eq!(online["online"], true);
    assert!(online["status"].is_object());
    assert_eq!(online["status"]["opencode_serve"], "running");
    assert_eq!(online["status"]["system"]["hostname"], "box1");
    assert_eq!(online["opencode_online"], true);
    assert_eq!(online["available"], 0);

    let offline = devices
        .iter()
        .find(|d| d["name"] == "dev-offline")
        .unwrap();
    assert_eq!(offline["online"], false);
    assert!(offline["reason"].is_string());
    assert_eq!(offline["opencode_online"], false);
    assert_eq!(offline["available"], -1);
}

/// `/api/me` rolls `available` up to `1` (green) only when bound + online
/// + opencode_online all hold; the test wires a registry with `bound=true`
/// against an online mock returning opencode_serve="running".
#[tokio::test]
#[serial]
async fn me_available_is_one_when_bound_and_running() {
    let sb = MockServer::start().await;
    let device = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/status"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"opencode_serve":"running","rathole":"running","system":{"hostname":"box"}}"#,
        ))
        .mount(&device)
        .await;

    sb_get(
        &sb,
        users_fs_path(),
        200,
        &serde_json::json!({
            "version": 1,
            "users": [{
                "id": TEST_USER_ID,
                "name": TEST_USER,
                "key": TEST_KEY,
                "cloud_ip": "127.0.0.1",
                "devices": [{
                    "desc": "d", "name": "d", "port": port_of(&device.uri()),
                    "pctype": "windows", "bound": true,
                    "public-url": host_of(&device.uri())
                }],
                "created_at": "2026-09-13T00:00:00+08:00",
                "updated_at": "2026-09-13T00:00:00+08:00"
            }]
        })
        .to_string(),
    )
    .await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = login_mounted(&app).await;

    let resp = send(&app, authed_get("/api/me", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let d = &body["devices"][0];
    assert_eq!(d["bound"], true);
    assert_eq!(d["online"], true);
    assert_eq!(d["opencode_online"], true);
    assert_eq!(d["available"], 1);
}

/// When /status answers but `opencode_serve` is anything other than the
/// literal `"running"` (e.g. `"stopped"`), opencode_online must be false
/// and available must be `0` (reachable but oc not running), regardless of
/// the bound flag.
#[tokio::test]
#[serial]
async fn me_available_is_zero_when_opencode_serve_stopped() {
    let sb = MockServer::start().await;
    let device = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/status"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"opencode_serve":"stopped","rathole":"running"}"#,
        ))
        .mount(&device)
        .await;

    sb_get(
        &sb,
        users_fs_path(),
        200,
        &serde_json::json!({
            "version": 1,
            "users": [{
                "id": TEST_USER_ID,
                "name": TEST_USER,
                "key": TEST_KEY,
                "cloud_ip": "127.0.0.1",
                "devices": [{
                    "desc": "d", "name": "d", "port": port_of(&device.uri()),
                    "pctype": "windows", "bound": true,
                    "public-url": host_of(&device.uri())
                }],
                "created_at": "2026-09-13T00:00:00+08:00",
                "updated_at": "2026-09-13T00:00:00+08:00"
            }]
        })
        .to_string(),
    )
    .await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = login_mounted(&app).await;

    let resp = send(&app, authed_get("/api/me", &cookie)).await;
    let body = body_json(resp).await;
    let d = &body["devices"][0];
    assert_eq!(d["online"], true, "/status answered");
    assert_eq!(d["opencode_online"], false, "opencode_serve != running");
    assert_eq!(d["available"], 0);
}

/// Regression: `/api/me`'s status probe MUST hit each device's own
/// `public_url` (admin-assigned per `UserDevice`), not a shared cloud
/// gateway. Otherwise every bound device collapses onto whichever
/// backend is up at the gateway and `online=true` for all of them —
/// the bug fixed earlier. The test mounts three device mocks (mac +
/// win answer /status with distinct hostnames, no-mock has no mock
/// mounted) and asserts each device surfaces its own state; the
/// "no-mock must be offline" assertion is the regression check.
#[tokio::test]
#[serial]
async fn me_probes_each_device_at_its_own_public_url() {
    let sb = MockServer::start().await;
    let mac = MockServer::start().await;
    let win = MockServer::start().await;
    let no_mock = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/status"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"opencode_serve":"running","rathole":"running","system":{"hostname":"mac-host"}}"#,
        ))
        .mount(&mac)
        .await;
    Mock::given(method("GET"))
        .and(path("/status"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"opencode_serve":"running","rathole":"running","system":{"hostname":"win-host"}}"#,
        ))
        .mount(&win)
        .await;

    sb_get(
        &sb,
        users_fs_path(),
        200,
        &users_json_with_public_urls(
            "127.0.0.1",
            &[
                ("dev-mac", mac.uri().as_str()),
                ("dev-win", win.uri().as_str()),
                ("dev-no-mock", no_mock.uri().as_str()),
            ],
        ),
    )
    .await;

    let app = test_app(build_state(&sb.uri()));
    let cookie = login_mounted(&app).await;

    let resp = send(&app, authed_get("/api/me", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let devices = body["devices"].as_array().expect("devices[]");
    assert_eq!(devices.len(), 3);

    let mac_d = devices.iter().find(|d| d["name"] == "dev-mac").unwrap();
    assert_eq!(mac_d["online"], true, "mac mock answered");
    assert_eq!(mac_d["status"]["system"]["hostname"], "mac-host");
    assert_eq!(mac_d["opencode_online"], true);
    // bound=false in the fixture → reachable-but-not-bound (0). The
    // available==1 (bound AND online AND running) path is covered by
    // me_available_is_one_when_bound_and_running.
    assert_eq!(mac_d["available"], 0);

    let win_d = devices.iter().find(|d| d["name"] == "dev-win").unwrap();
    assert_eq!(win_d["online"], true, "win mock answered");
    assert_eq!(win_d["status"]["system"]["hostname"], "win-host");
    assert_eq!(win_d["opencode_online"], true);
    assert_eq!(win_d["available"], 0);

    // THE regression assertion: with the bug present, the no-mock device
    // would also report online=true because all probes funneled through
    // the same shared cloud gateway. After the fix, it must surface as
    // offline — its own public_url returns connection refused.
    let off_d = devices.iter().find(|d| d["name"] == "dev-no-mock").unwrap();
    assert_eq!(
        off_d["online"], false,
        "no-mock device must NOT inherit online from a sibling device"
    );
    assert_eq!(off_d["opencode_online"], false);
    assert_eq!(off_d["available"], -1);
    assert!(off_d["reason"].is_string());
}
