//! Full-chain BFF integration tests.
//!
//! Each test hand-builds an `AppState` (see `common::build_state`) around
//! fresh wiremock stand-ins for SilverBullet and the per-device `oc serve`
//! instances, mounts the real `build_router` router, and drives it with
//! `tower::ServiceExt::oneshot` — no real ports, no env vars, no live SB
//! login. All tests run serially (`serial_test::serial`) so shared ambient
//! state can never bleed across cases.

mod common;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use common::*;
use serial_test::serial;
use wiremock::matchers::{header as header_matcher, method, path};
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
// 5-7. /api/devices
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn devices_requires_session() {
    let sb = MockServer::start().await;
    let app = test_app(build_state(&sb.uri()));

    let resp = send(&app, req(Method::GET, "/api/devices")).await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "unauthorized");
}

#[tokio::test]
#[serial]
async fn devices_returns_empty_when_no_devices() {
    let sb = MockServer::start().await;
    // devices.json does not exist yet → the cache falls back to an empty list.
    sb_get_404(&sb, &devices_fs_path()).await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    let resp = send(&app, authed_get("/api/devices", &cookie)).await;

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await, serde_json::json!([]));
}

#[tokio::test]
#[serial]
async fn devices_returns_online_status_from_health() {
    let sb = MockServer::start().await;
    let online_dev = MockServer::start().await;
    let offline_dev = MockServer::start().await;

    // The BFF probes `<public_url>/health` per device: 2xx → online.
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&online_dev)
        .await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&offline_dev)
        .await;

    let devices_json = serde_json::json!({
        "version": 1,
        "devices": [
            {"pctype": "macos", "pcname": "dev-online", "public_url": online_dev.uri(), "oc_serve_port": 4040},
            {"pctype": "windows", "pcname": "dev-offline", "public_url": offline_dev.uri(), "oc_serve_port": 4040},
        ]
    })
    .to_string();
    sb_get(&sb, &devices_fs_path(), 200, &devices_json).await;

    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    let resp = send(&app, authed_get("/api/devices", &cookie)).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let devices = body.as_array().expect("devices list");
    assert_eq!(devices.len(), 2);

    let online = devices
        .iter()
        .find(|d| d["pcname"] == "dev-online")
        .expect("dev-online present");
    assert_eq!(online["online"], true);
    assert_eq!(online["pcname_b64"], b64url("dev-online"));
    assert_eq!(online["portal_base"], "https://oc.example.com");

    let offline = devices
        .iter()
        .find(|d| d["pcname"] == "dev-offline")
        .expect("dev-offline present");
    assert_eq!(offline["online"], false);
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
    // location: serv/opencode/{user_id}/{pctype}/{device-name}/path-list.md
    // — the tester registry entry assigns pctype "windows" and carries no
    // device-name yet, so the segment falls back to the service name.
    sb_get(
        &sb,
        "serv/opencode/100001/windows/dev-one/path-list.md",
        200,
        r#"[
            {"path":"D:/work/api","sections":["s1"],"createdAt":"2026-09-01T10:00:00+08:00","lastOpenedAt":"2026-09-02T10:00:00+08:00"},
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
            {"path":"D:/work/api","sections":["s1","s2"],"createdAt":"2026-09-01T10:00:00+08:00","lastOpenedAt":"2026-09-03T11:00:00+08:00"},
            {"path":"D:/work/other","sections":["s9"]}
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
    // updatedAt carries the path entry's lastOpenedAt.
    assert_eq!(body[0]["updatedAt"], "2026-09-03T11:00:00+08:00");
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
        &devices_fs_path(),
        200,
        &one_device_json("macos", "dev-one", &device.uri()),
    )
    .await;
    // The jump URL is built from devices.json's public_url (the device
    // mock itself) — no config.md overrides any more.

    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

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
        auth_token_query(TEST_USER, TEST_PASS)
    );
    assert_eq!(body["jump_url"], expected_jump);
}

/// `?auth_token=` value the BFF embeds for the unified test credentials
/// (standard base64 of `user:password`, percent-encoded — see
/// `jump::append_auth_token`).
fn auth_token_query(user: &str, pass: &str) -> String {
    use base64::Engine;
    let token = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", user, pass));
    urlencoding::encode(&token).to_string()
}

// ---------------------------------------------------------------------------
// 14. seed
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn seed_writes_demo_device() {
    let sb = MockServer::start().await;
    // Start with no devices.json → seed must create the file content.
    sb_get_404(&sb, &devices_fs_path()).await;
    sb_put_204(&sb, &devices_fs_path()).await;

    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/devices/seed",
            &serde_json::json!({
                "pcname": "dev-seed",
                "public_url": "http://127.0.0.1:9999",
                "pctype": "windows",
            }),
            Some(&cookie),
        ),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await, serde_json::json!({"ok": true}));

    // The demo device must have been written to SB's devices.json.
    sb.verify().await;
    let devices_http_path = format!("/.fs/{}", devices_fs_path());
    let writes: Vec<_> = sb
        .received_requests()
        .await
        .expect("wiremock records requests")
        .into_iter()
        .filter(|r| r.method.as_str() == "PUT" && r.url.path() == devices_http_path)
        .collect();
    assert_eq!(writes.len(), 1, "exactly one devices.json write");
    let written: serde_json::Value = serde_json::from_slice(&writes[0].body).unwrap();
    let devices = written["devices"].as_array().expect("devices array");
    let seeded = devices
        .iter()
        .find(|d| d["pcname"] == "dev-seed")
        .expect("demo device written");
    assert_eq!(seeded["pctype"], "windows");
    assert_eq!(seeded["public_url"], "http://127.0.0.1:9999");
}

// ---------------------------------------------------------------------------
// 15. device delete
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn device_delete_removes_seeded_device() {
    let sb = MockServer::start().await;
    sb_get(
        &sb,
        &devices_fs_path(),
        200,
        &serde_json::json!({
            "version": 1,
            "devices": [
                {"pctype": "windows", "pcname": "dev-seed", "public_url": "http://127.0.0.1:9999", "oc_serve_port": 4040},
                {"pctype": "macos", "pcname": "auto-reg", "public_url": "http://127.0.0.1:8888", "oc_serve_port": 4040, "version": "1.18.30"}
            ]
        })
        .to_string(),
    )
    .await;
    sb_put_204(&sb, &devices_fs_path()).await;

    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    // Delete the seeded (manually added) device.
    let resp = send(
        &app,
        Request::builder()
            .method(Method::DELETE)
            .uri("/api/devices/windows/dev-seed")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await, serde_json::json!({"ok": true}));

    // The PUT written back to SB keeps the auto-registered device only.
    let devices_http_path = format!("/.fs/{}", devices_fs_path());
    let writes: Vec<_> = sb
        .received_requests()
        .await
        .expect("wiremock records requests")
        .into_iter()
        .filter(|r| r.method.as_str() == "PUT" && r.url.path() == devices_http_path)
        .collect();
    assert_eq!(writes.len(), 1, "exactly one devices.json write");
    let written: serde_json::Value = serde_json::from_slice(&writes[0].body).unwrap();
    let devices = written["devices"].as_array().expect("devices array");
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0]["pcname"], "auto-reg");
}

#[tokio::test]
#[serial]
async fn device_delete_unknown_returns_404() {
    let sb = MockServer::start().await;
    sb_get(
        &sb,
        &devices_fs_path(),
        200,
        &serde_json::json!({"version": 1, "devices": []}).to_string(),
    )
    .await;

    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    let resp = send(
        &app,
        Request::builder()
            .method(Method::DELETE)
            .uri("/api/devices/macos/ghost")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial]
async fn device_delete_requires_session() {
    let sb = MockServer::start().await;
    let app = test_app(build_state(&sb.uri()));

    let resp = send(
        &app,
        Request::builder()
            .method(Method::DELETE)
            .uri("/api/devices/macos/any")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// 16. device detail
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn device_detail_reports_identity_and_unified_credentials() {
    let sb = MockServer::start().await;
    sb_get(
        &sb,
        &devices_fs_path(),
        200,
        &one_device_json("windows", "HomeWin", "http://127.0.0.1:9464"),
    )
    .await;

    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    let resp = send(
        &app,
        authed_get("/api/devices/windows/HomeWin/detail", &cookie),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["pctype"], "windows");
    assert_eq!(body["pcname"], "HomeWin");
    assert_eq!(body["public_url"], "http://127.0.0.1:9464");
    assert_eq!(body["username"], TEST_USER);
    assert_eq!(body["password"], TEST_PASS);
}

#[tokio::test]
#[serial]
async fn device_detail_prefers_cached_credentials() {
    let sb = MockServer::start().await;
    let device = MockServer::start().await;
    // /project mock: 200 only for the device-user pair, 401 otherwise.
    // wiremock checks mocks in mounting order (first mounted wins), so
    // the specific header-matching mock must come BEFORE the catch-all.
    Mock::given(method("GET"))
        .and(path("/project"))
        .and(header_matcher(
            "authorization",
            format!("Basic {}", STANDARD.encode("device-user:device-pass")),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&device)
        .await;
    Mock::given(method("GET"))
        .and(path("/project"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&device)
        .await;
    sb_get(
        &sb,
        &devices_fs_path(),
        200,
        &one_device_json("windows", "HomeWin", &device.uri()),
    )
    .await;

    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    // Before authorizing: unified pair.
    let resp = send(
        &app,
        authed_get("/api/devices/windows/HomeWin/detail", &cookie),
    )
    .await;
    assert_eq!(body_json(resp).await["username"], TEST_USER);

    // Authorize with the device's own pair.
    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/devices/windows/HomeWin/auth",
            &serde_json::json!({"username": "device-user", "password": "device-pass"}),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // After: detail reports the cached pair.
    let resp = send(
        &app,
        authed_get("/api/devices/windows/HomeWin/detail", &cookie),
    )
    .await;
    let body = body_json(resp).await;
    assert_eq!(body["username"], "device-user");
    assert_eq!(body["password"], "device-pass");
}

#[tokio::test]
#[serial]
async fn device_detail_requires_session() {
    let sb = MockServer::start().await;
    let app = test_app(build_state(&sb.uri()));

    let resp = send(&app, req(Method::GET, "/api/devices/windows/HomeWin/detail")).await;
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

    // Registry with cloud_ip=127.0.0.1 and per-device ports matching the
    // mock servers — the me handler probes http://{cloud_ip}:{port}/status.
    let app = test_app(
        build_state_with_cloud_ip(
            &sb,
            "127.0.0.1",
            &[
                ("dev-online", online_dev.uri().as_str()),
                ("dev-offline", offline_dev.uri().as_str()),
            ],
        )
        .await,
    );
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

    let offline = devices
        .iter()
        .find(|d| d["name"] == "dev-offline")
        .unwrap();
    assert_eq!(offline["online"], false);
    assert!(offline["reason"].is_string());
}
