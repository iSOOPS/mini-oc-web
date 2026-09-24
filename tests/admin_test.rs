//! Admin (super-admin, SB password) API tests: login, rate limiting,
//! guard on every /api/admin route, server info, and the user CRUD flow
//! against the wiremock SilverBullet.

mod common;

use axum::http::{header, Method, StatusCode};
use common::*;
use serial_test::serial;
use wiremock::MockServer;

#[tokio::test]
#[serial]
async fn admin_login_wrong_password_returns_401() {
    let sb = MockServer::start().await;
    let app = test_app(build_state(&sb.uri()));

    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/admin/login",
            &serde_json::json!({"password": "not-the-sb-password"}),
            None,
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert!(resp.headers().get(header::SET_COOKIE).is_none());
}

#[tokio::test]
#[serial]
async fn admin_login_rate_limited_after_5_failures() {
    let sb = MockServer::start().await;
    let app = test_app(build_state(&sb.uri()));

    for _ in 0..5 {
        let resp = send(
            &app,
            json_req(
                Method::POST,
                "/api/admin/login",
                &serde_json::json!({"password": "wrong"}),
                None,
            ),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/admin/login",
            &serde_json::json!({"password": SB_PASS}),
            None,
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
#[serial]
async fn admin_routes_reject_missing_admin_cookie() {
    let sb = MockServer::start().await;
    let app = test_app(build_state(&sb.uri()));

    let valid_user =
        serde_json::json!({"name": "x", "devices": [{"desc": "设备甲", "name": "a", "port": 1, "pctype": "windows"}]});
    for (method, uri, body) in [
        (Method::GET, "/api/admin/info", serde_json::json!({})),
        (Method::GET, "/api/admin/users", serde_json::json!({})),
        (Method::POST, "/api/admin/users", valid_user.clone()),
        (
            Method::PUT,
            "/api/admin/users/xyz",
            valid_user.clone(),
        ),
        (Method::DELETE, "/api/admin/users/xyz", serde_json::json!({})),
        (
            Method::POST,
            "/api/admin/users/xyz/regenerate-key",
            serde_json::json!({}),
        ),
    ] {
        let resp = send(&app, json_req(method.clone(), uri, &body, None)).await;
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {uri} must require the admin cookie"
        );
    }
}

/// A logged-in USER cookie must not open admin routes either.
#[tokio::test]
#[serial]
async fn admin_routes_reject_user_session_cookie() {
    let sb = MockServer::start().await;
    let app = test_app(build_state(&sb.uri()));
    let user_cookie = login(&app, &sb).await;

    let resp = send(&app, authed_get("/api/admin/users", &user_cookie)).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[serial]
async fn admin_info_reports_server_basics() {
    let sb = MockServer::start().await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = admin_login(&app).await;

    // Unmatched SB reads 404 → empty registries; info must still answer.
    let resp = send(&app, authed_get("/api/admin/info", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["web_port"], 8100);
    assert_eq!(body["web_bind"], "127.0.0.1");
    assert_eq!(body["portal_base"], "https://oc.example.com");
    assert_eq!(body["sb_user"], SB_USER);
    assert_eq!(body["user_count"], 0);
    assert_eq!(body["device_count"], 0);
    assert_eq!(
        body["sb_password"].as_str().map(str::len),
        Some(SB_PASS.len()),
        "sb_password is exposed to the admin console"
    );
    assert!(
        body["local_ips"].as_array().is_some(),
        "local_ips must be an array"
    );
    assert!(
        body["uptime_secs"].as_u64().is_some(),
        "uptime_secs must be a number"
    );
    assert!(
        body["version"].as_str().is_some_and(|s| !s.is_empty()),
        "version must be reported"
    );
}

#[tokio::test]
#[serial]
async fn admin_create_user_generates_key_and_writes_registry() {
    let sb = MockServer::start().await;
    // Empty registry on first load (missing document).
    sb_put_204(&sb, users_fs_path()).await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = admin_login(&app).await;

    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/admin/users",
            &serde_json::json!({
                "name": "alice",
                "devices": [
                    {"desc": "Alice 的 Mac", "name": "alice-mac", "port": 9464, "pctype": "macos", "bound": true},
                    {"desc": "Alice 的 Windows", "name": "alice-win", "port": 4040, "pctype": "windows"},
                ],
                "sb": {
                    "base_url": "https://md.isoops.com",
                    "username": "alice",
                    "password": "sb-alice-pass",
                },
            }),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let user = body_json(resp).await;
    assert_eq!(user["name"], "alice");
    assert!(
        user.get("oc_serve_port").is_none(),
        "oc_serve_port must be gone from the user model"
    );
    let key = user["key"].as_str().expect("created user must carry a key");
    assert_eq!(key.len(), 32, "key is 32 chars: {key}");
    assert!(
        key.chars().all(|c| c.is_ascii_alphanumeric()),
        "key must contain no symbols: {key}"
    );
    let id = user["id"].as_str().expect("created user must carry an id");
    assert_eq!(id.len(), 6, "id is a 6-digit random number: {id}");
    assert!(
        id.chars().all(|c| c.is_ascii_digit()),
        "id must be digits only: {id}"
    );

    // The registry written to SB carries the new user (including the
    // admin-assigned SB storage config).
    let puts: Vec<_> = sb
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.method.as_str() == "PUT" && r.url.path() == "/.fs/web/opencode/config.md")
        .collect();
    assert_eq!(puts.len(), 1);
    let written: serde_json::Value = serde_json::from_slice(&puts[0].body).unwrap();
    assert_eq!(written["users"].as_array().unwrap().len(), 1);
    assert_eq!(written["users"][0]["name"], "alice");
    assert_eq!(written["users"][0]["key"], key);
    assert_eq!(written["users"][0]["sb"]["base_url"], "https://md.isoops.com");
    assert_eq!(written["users"][0]["sb"]["username"], "alice");
    assert_eq!(written["users"][0]["sb"]["password"], "sb-alice-pass");
    // bound persists into the registry; omitted bound defaults to false.
    assert_eq!(written["users"][0]["devices"][0]["bound"], true);
    assert_eq!(written["users"][0]["devices"][1]["bound"], false);
}

/// Creation mints a 6-digit random id that does not duplicate any id in
/// the freshly-fetched remote registry (neither existing 6-digit ids nor
/// legacy uuids).
#[tokio::test]
#[serial]
async fn admin_create_user_id_is_6_digits_and_unique_in_registry() {
    let sb = MockServer::start().await;
    let registry = serde_json::json!({
        "version": 1,
        "users": [
            {
                "id": "123456",
                "name": "taken-six-digit",
                "key": "k1234567890123456789012345678901",
                "devices": [{"name": "dev-one", "port": 4040}],
            },
            {
                "id": TEST_USER_ID,
                "name": TEST_USER,
                "key": "k2345678901234567890123456789012",
                "devices": [{"name": "dev-two", "port": 4040}],
            },
        ]
    })
    .to_string();
    sb_get(&sb, users_fs_path(), 200, &registry).await;
    sb_put_204(&sb, users_fs_path()).await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = admin_login(&app).await;

    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/admin/users",
            &serde_json::json!({
                "name": "alice",
                "devices": [{"desc": "Alice 的 Mac", "name": "alice-mac", "port": 9465, "oc-port": 9464, "pctype": "macos", "public-url": "https://oc-alice.example.com"}],
            }),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let user = body_json(resp).await;
    let id = user["id"].as_str().expect("created user must carry an id");
    assert_eq!(id.len(), 6, "id must be 6 digits: {id}");
    assert!(
        id.chars().all(|c| c.is_ascii_digit()),
        "id must be digits only: {id}"
    );
    assert_ne!(id, "123456", "must not collide with the existing 6-digit id");
    assert_ne!(id, TEST_USER_ID, "must not collide with the other user id");

    // The written registry keeps every id unique and carries the new
    // user's default sb config.
    let puts: Vec<_> = sb
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.method.as_str() == "PUT" && r.url.path() == "/.fs/web/opencode/config.md")
        .collect();
    assert_eq!(puts.len(), 1);
    let written: serde_json::Value = serde_json::from_slice(&puts[0].body).unwrap();
    let users = written["users"].as_array().unwrap();
    assert_eq!(users.len(), 3);
    let alice = users.iter().find(|u| u["name"] == "alice").unwrap();
    assert_eq!(alice["id"], id);
    assert_eq!(alice["sb"]["base_url"], "https://md.isoops.com");
    let mut ids: Vec<&str> = users.iter().filter_map(|u| u["id"].as_str()).collect();
    ids.sort_unstable();
    let count = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), count, "ids in the written registry must be unique");
}

#[tokio::test]
#[serial]
async fn admin_create_user_validates_input() {
    let sb = MockServer::start().await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = admin_login(&app).await;

    let cases = [
        serde_json::json!({"name": "bad name", "devices": [{"desc": "甲", "name": "a", "port": 1, "pctype": "windows"}]}),
        serde_json::json!({"name": "", "devices": [{"desc": "甲", "name": "a", "port": 1, "pctype": "windows"}]}),
        serde_json::json!({"name": "alice", "devices": [{"name": "a", "port": 0, "pctype": "windows"}]}),
        serde_json::json!({"name": "alice", "devices": []}),
        serde_json::json!({"name": "alice", "devices": [{"name": "bad name", "port": 1, "pctype": "windows"}]}),
        serde_json::json!({"name": "alice", "devices": [{"name": "Under_score", "port": 1, "pctype": "windows"}]}),
        serde_json::json!({"name": "alice", "devices": [{"desc": "甲", "name": "a", "port": 1, "pctype": "windows"}, {"desc": "乙", "name": "a", "port": 2, "pctype": "windows"}]}),
        // desc 校验：缺失 / 空白 / 含控制字符 → 400。
        serde_json::json!({"name": "alice", "devices": [{"name": "a", "port": 1, "pctype": "windows"}]}),
        serde_json::json!({"name": "alice", "devices": [{"desc": "   ", "name": "a", "port": 1, "pctype": "windows"}]}),
        serde_json::json!({"name": "alice", "devices": [{"desc": "a\nb", "name": "a", "port": 1, "pctype": "windows"}]}),
        // pctype 枚举校验：缺失 / 非法值 → 400。
        serde_json::json!({"name": "alice", "devices": [{"desc": "甲", "name": "a", "port": 1}]}),
        serde_json::json!({"name": "alice", "devices": [{"desc": "甲", "name": "a", "port": 1, "pctype": "linux"}]}),
    ];
    for body in cases {
        let resp = send(
            &app,
            json_req(Method::POST, "/api/admin/users", &body, Some(&cookie)),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "body {body} must be rejected"
        );
    }
}

#[tokio::test]
#[serial]
async fn admin_list_update_regenerate_delete_flow() {
    let sb = MockServer::start().await;
    // The registry the SB mock serves for loads inside update/regenerate/
    // delete (each handler loads once — the mock serves it every time;
    // PUTs are NOT persisted). The preset SB config lets the "omit sb
    // keeps the stored value" assertion work without persistence: if the
    // update handler wiped sb on omission, the preset would be gone.
    let mut registry: serde_json::Value =
        serde_json::from_str(&users_json(DEFAULT_DEVICES)).unwrap();
    registry["users"][0]["sb"] = serde_json::json!({
        "base_url": "https://md.isoops.com",
        "username": "preset-user",
        "password": "preset-pass",
    });
    sb_get(&sb, users_fs_path(), 200, &registry.to_string()).await;
    sb_put_204(&sb, users_fs_path()).await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = admin_login(&app).await;

    // List: the default user with full key (admin must be able to
    // redistribute keys).
    let resp = send(&app, authed_get("/api/admin/users", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let list = body_json(resp).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["key"], TEST_KEY);

    // Update: rename + new device list + reassigned SB config; key must
    // be preserved.
    let resp = send(
        &app,
        json_req(
            Method::PUT,
            &format!("/api/admin/users/{}", TEST_USER_ID),
            &serde_json::json!({
                "name": "renamed",
                "devices": [{"desc": "一号机", "name": "dev-one", "port": 8200, "pctype": "windows", "public-url": "https://dev-one.example.com"}],
                "sb": {
                    "base_url": "https://md.isoops.com",
                    "username": "renamed-user",
                    "password": "sb-renamed-pass",
                },
            }),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let updated = body_json(resp).await;
    assert_eq!(updated["name"], "renamed");
    assert_eq!(updated["devices"][0]["port"], 8200);
    assert_eq!(updated["sb"]["username"], "renamed-user");
    assert_eq!(updated["sb"]["password"], "sb-renamed-pass");
    assert_eq!(updated["key"], TEST_KEY, "update must preserve the key");

    // Update WITHOUT `sb`: the stored SB config must be preserved (sb is
    // optional in UserBody; omitting it keeps the stored value instead
    // of wiping credentials). The mock does not persist the previous
    // PUT, so this load re-reads the PRESET config — if the handler
    // wiped sb on omission, the preset would be gone.
    let resp = send(
        &app,
        json_req(
            Method::PUT,
            &format!("/api/admin/users/{}", TEST_USER_ID),
            &serde_json::json!({
                "name": "renamed",
                "devices": [{"desc": "一号机", "name": "dev-one", "port": 8200, "pctype": "windows", "public-url": "https://dev-one.example.com"}],
            }),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let updated = body_json(resp).await;
    assert_eq!(
        updated["sb"]["username"], "preset-user",
        "omitting sb in update must keep the stored SB config"
    );
    assert_eq!(updated["sb"]["password"], "preset-pass");

    // Regenerate: brand-new 32-char key.
    let resp = send(
        &app,
        json_req(
            Method::POST,
            &format!("/api/admin/users/{}/regenerate-key", TEST_USER_ID),
            &serde_json::json!({}),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let new_key = body_json(resp).await["key"].as_str().unwrap().to_string();
    assert_eq!(new_key.len(), 32);
    assert_ne!(new_key, TEST_KEY);

    // Delete: removes the user (PUT with empty users array).
    let resp = send(
        &app,
        json_req(
            Method::DELETE,
            &format!("/api/admin/users/{}", TEST_USER_ID),
            &serde_json::json!({}),
            Some(&cookie),
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
    let last: serde_json::Value = serde_json::from_slice(&puts.last().unwrap().body).unwrap();
    assert_eq!(
        last["users"].as_array().unwrap().len(),
        0,
        "final registry must be empty"
    );
    // NOTE: no "delete again → 404" assertion here — the wiremock SB mock
    // always re-serves the original registry document (PUTs are not
    // persisted), so a re-load resurrects the user. The unknown-id 404
    // path is covered by `admin_update_unknown_user_returns_404` and the
    // delete handler shares the same lookup.
}

#[tokio::test]
#[serial]
async fn admin_update_unknown_user_returns_404() {
    let sb = MockServer::start().await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = admin_login(&app).await;

    let resp = send(
        &app,
        json_req(
            Method::PUT,
            "/api/admin/users/00000000-0000-4000-8000-000000000000",
            &serde_json::json!({"name": "x", "devices": [{"desc": "设备甲", "name": "a", "port": 1, "pctype": "windows", "public-url": "https://a.example.com"}]}),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
