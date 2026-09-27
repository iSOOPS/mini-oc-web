//! User self-service endpoints (settings page backing) + device status
//! probing through the cloud tunnel.

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use serial_test::serial;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A registry document with explicit cloud_ip + a device on `port`,
/// optionally carrying an admin-assigned tunnel address (any port in
/// `public_url` is stripped — ports live in the port/oc-port fields).
fn users_json_with(ip: &str, port: u16, public_url: &str) -> String {
    serde_json::json!({
        "version": 1,
        "users": [{
            "id": TEST_USER_ID,
            "name": TEST_USER,
            "key": TEST_KEY,
            "cloud_ip": ip,
            "devices": [{"name": "dev-x", "port": port, "public-url": host_of(public_url)}],
        }]
    })
    .to_string()
}

#[tokio::test]
#[serial]
async fn login_stamps_last_used_at_into_registry() {
    let sb = MockServer::start().await;
    sb_get(&sb, users_fs_path(), 200, &users_json(&["dev-one"])).await;
    sb_put_204(&sb, users_fs_path()).await;
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

    let puts: Vec<_> = sb
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.method.as_str() == "PUT" && r.url.path() == "/.fs/web/opencode/config.md")
        .collect();
    assert_eq!(puts.len(), 1, "login writes last_used_at back once");
    let written: serde_json::Value = serde_json::from_slice(&puts[0].body).unwrap();
    assert!(
        written["users"][0]["last_used_at"]
            .as_str()
            .is_some_and(|s| !s.is_empty()),
        "last_used_at must be stamped"
    );
}

#[tokio::test]
#[serial]
async fn me_reports_last_used_at() {
    let sb = MockServer::start().await;
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
                "last_used_at": "2026-09-13T08:00:00+08:00",
                "devices": [{"name": "dev-x", "port": 4040}],
            }]
        })
        .to_string(),
    )
    .await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    let resp = send(&app, authed_get("/api/me", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    // cloud_ip is intentionally absent from /api/me now: it is a
    // device→cloud callback address (served via /api/user/info), not a
    // portal-facing field, and the settings-page editor is gone.
    assert!(body.get("cloud_ip").is_none(), "cloud_ip must not leak into /api/me");
    assert_eq!(body["last_used_at"], "2026-09-13T08:00:00+08:00");
}

#[tokio::test]
#[serial]
async fn me_key_reveals_own_login_key() {
    let sb = MockServer::start().await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    let resp = send(&app, authed_get("/api/me/key", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await["key"], TEST_KEY);

    // Without a session: 401.
    let resp = send(&app, req(Method::GET, "/api/me/key")).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[serial]
async fn me_rename_updates_and_stays_logged_in() {
    let sb = MockServer::start().await;
    sb_put_204(&sb, users_fs_path()).await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/me/name",
            &serde_json::json!({"name": "renamed-me"}),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await["name"], "renamed-me");

    // Same cookie still valid (id-based). NOTE: the wiremock SB mock does
    // not persist the PUT, so a fresh registry load re-serves the OLD
    // document — we assert the session survives the rename, not the
    // re-read value.
    let resp = send(&app, authed_get("/api/me", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK, "session survives rename");

    // Invalid name rejected.
    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/me/name",
            &serde_json::json!({"name": "bad name"}),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[serial]
async fn device_status_probes_cloud_endpoint() {
    let device = MockServer::start().await;
    let port = device.uri().split(':').next_back().unwrap().parse::<u16>().unwrap();

    Mock::given(method("GET"))
        .and(path("/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({
                "opencode_serve": "running",
                "rathole": "running",
                "started_at": null,
                "system": {"os": "windows", "arch": "x86_64", "hostname": "dev-x-host"},
                "version": "0.1.0"
            }),
        ))
        .expect(1)
        .mount(&device)
        .await;

    let sb = MockServer::start().await;
    sb_get(
        &sb,
        users_fs_path(),
        200,
        &users_json_with("127.0.0.1", port, &device.uri()),
    )
    .await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    let resp = send(
        &app,
        authed_get(&format!("/api/device-status/{port}"), &cookie),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["online"], true);
    assert_eq!(body["status"]["opencode_serve"], "running");
    assert_eq!(body["status"]["system"]["hostname"], "dev-x-host");

    // A port outside the user's device list is forbidden.
    let resp = send(
        &app,
        authed_get("/api/device-status/9999", &cookie),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    device.verify().await;
}

#[tokio::test]
#[serial]
async fn device_status_reports_offline_for_unreachable_url() {
    // The device URL points at a dead port — the probe must answer
    // online:false with a network-level reason, not 5xx.
    let sb = MockServer::start().await;
    sb_get(
        &sb,
        users_fs_path(),
        200,
        &users_json_with("127.0.0.1", 59999, "http://127.0.0.1:1"),
    )
    .await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    let resp = send(&app, authed_get("/api/device-status/59999", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["online"], false);
    let reason = body["reason"].as_str().unwrap_or_default();
    assert!(!reason.is_empty(), "offline reason must be present");
    assert!(
        !reason.contains("not configured"),
        "a configured URL must be probed, got reason: {reason}"
    );
}

#[tokio::test]
#[serial]
async fn device_status_without_url_reports_not_configured() {
    // Legacy entry with an empty public-url: no probe attempt is made —
    // the cloud_ip user setting is a device→cloud callback address and
    // must never be borrowed as a probe target.
    let sb = MockServer::start().await;
    sb_get(
        &sb,
        users_fs_path(),
        200,
        &users_json_with("127.0.0.1", 59999, ""),
    )
    .await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    let resp = send(&app, authed_get("/api/device-status/59999", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["online"], false);
    assert!(
        body["reason"]
            .as_str()
            .is_some_and(|r| r.contains("not configured")),
        "empty public-url must report 'not configured', got: {}",
        body["reason"]
    );
}

#[tokio::test]
#[serial]
async fn me_reports_not_configured_for_empty_device_url() {
    let sb = MockServer::start().await;
    sb_get(
        &sb,
        users_fs_path(),
        200,
        &users_json_with("127.0.0.1", 59998, ""),
    )
    .await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    let resp = send(&app, authed_get("/api/me", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let dev = &body["devices"][0];
    assert_eq!(dev["online"], false);
    assert_eq!(dev["available"], -1);
    assert!(
        dev["reason"]
            .as_str()
            .is_some_and(|r| r.contains("not configured")),
        "empty public-url must surface a 'not configured' reason, got: {}",
        dev["reason"]
    );
}

/// /api/me carries the unique id and the sb config block.
#[tokio::test]
#[serial]
async fn me_reports_id_and_sb_config() {
    let sb = MockServer::start().await;
    sb_get(
        &sb,
        users_fs_path(),
        200,
        &serde_json::json!({
            "version": 1,
            "users": [{
                "id": "123456",
                "name": TEST_USER,
                "key": TEST_KEY,
                "cloud_ip": "127.0.0.1",
                "devices": [{"name": "dev-x", "port": 4040}],
                "sb": {
                    "base_url": "https://sb.example.com",
                    "username": "samuel",
                    "password": "sb-pass"
                }
            }]
        })
        .to_string(),
    )
    .await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    let resp = send(&app, authed_get("/api/me", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["id"], "123456");
    assert_eq!(body["sb"]["base_url"], "https://sb.example.com");
    assert_eq!(body["sb"]["username"], "samuel");
    assert_eq!(body["sb"]["password"], "sb-pass");
    assert!(
        body.get("key").is_none(),
        "/api/me never leaks key material"
    );
}

/// Self-service sb config update: persisted into the user's registry
/// entry and validated (bad scheme rejected).
#[tokio::test]
#[serial]
async fn me_sb_updates_registry_with_validation() {
    let sb = MockServer::start().await;
    sb_put_204(&sb, users_fs_path()).await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = login(&app, &sb).await;

    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/me/sb",
            &serde_json::json!({
                "base_url": "https://sb.example.com",
                "username": "samuel",
                "password": "sb-secret"
            }),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["ok"], true);
    assert_eq!(body["sb"]["base_url"], "https://sb.example.com");
    assert_eq!(body["sb"]["username"], "samuel");

    // The written registry carries the sb block on the user entry.
    // (2 PUTs: login's last_used_at stamp + this save.)
    let puts: Vec<_> = sb
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.method.as_str() == "PUT" && r.url.path() == "/.fs/web/opencode/config.md")
        .collect();
    assert_eq!(puts.len(), 2, "login stamp + sb save");
    let written: serde_json::Value = serde_json::from_slice(&puts[1].body).unwrap();
    assert_eq!(written["users"][0]["sb"]["base_url"], "https://sb.example.com");
    assert_eq!(written["users"][0]["sb"]["password"], "sb-secret");

    // Non-http scheme / whitespace / empty rejected.
    for bad in ["", "ftp://sb.example.com", "https://sb.example.com/x y"] {
        let resp = send(
            &app,
            json_req(
                Method::POST,
                "/api/me/sb",
                &serde_json::json!({
                    "base_url": bad,
                    "username": "samuel",
                    "password": "sb-secret"
                }),
                Some(&cookie),
            ),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "base_url {bad:?} must be rejected"
        );
    }

    // Without a session: 401.
    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/me/sb",
            &serde_json::json!({
                "base_url": "https://sb.example.com",
                "username": "",
                "password": ""
            }),
            None,
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// POST /api/user/info: full profile lookup by login key — no cookie.
#[tokio::test]
#[serial]
async fn user_info_by_key_returns_full_profile() {
    let sb = MockServer::start().await;
    sb_get(
        &sb,
        users_fs_path(),
        200,
        &serde_json::json!({
            "version": 1,
            "users": [{
                "id": "654321",
                "name": TEST_USER,
                "key": TEST_KEY,
                "cloud_ip": "1.2.3.4",
                "devices": [{"name": "dev-x", "port": 4040}],
                "sb": {
                    "base_url": "https://sb.example.com",
                    "username": "samuel",
                    "password": "sb-secret"
                },
                "created_at": "2026-09-13T00:00:00+08:00",
                "updated_at": "2026-09-13T00:00:00+08:00"
            }]
        })
        .to_string(),
    )
    .await;
    let app = test_app(build_state(&sb.uri()));

    let resp = send(
        &app,
        json_req(Method::POST, "/api/user/info", &serde_json::json!({"key": TEST_KEY}), None),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["id"], "654321");
    assert_eq!(body["name"], TEST_USER);
    assert_eq!(body["key"], TEST_KEY, "full profile includes the key itself");
    assert_eq!(body["cloud_ip"], "1.2.3.4");
    assert_eq!(body["devices"][0]["name"], "dev-x");
    assert_eq!(body["devices"][0]["port"], 4040);
    assert_eq!(body["sb"]["base_url"], "https://sb.example.com");
    assert_eq!(body["sb"]["username"], "samuel");
    assert_eq!(body["sb"]["password"], "sb-secret");
    assert_eq!(body["created_at"], "2026-09-13T00:00:00+08:00");

    // Bad key: 401, and each failure counts toward the rate limit.
    let resp = send(
        &app,
        json_req(Method::POST, "/api/user/info", &serde_json::json!({"key": "wrong"}), None),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Brute-force protection: after 5 bad keys the source IP is locked out
/// — even a CORRECT key is rejected with 429 for the window.
#[tokio::test]
#[serial]
async fn user_info_by_key_rate_limited_after_5_failures() {
    let sb = MockServer::start().await;
    sb_get(&sb, users_fs_path(), 200, &users_json(DEFAULT_DEVICES)).await;
    let app = test_app(build_state(&sb.uri()));

    for i in 0..5 {
        let resp = send(
            &app,
            json_req(
                Method::POST,
                "/api/user/info",
                &serde_json::json!({"key": format!("wrong-{i}")}),
                None,
            ),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "attempt {i}");
    }

    // The IP is now locked: even the VALID key gets 429, not 200 —
    // an attacker cannot keep guessing after the failure budget is spent.
    let resp = send(
        &app,
        json_req(Method::POST, "/api/user/info", &serde_json::json!({"key": TEST_KEY}), None),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
}
