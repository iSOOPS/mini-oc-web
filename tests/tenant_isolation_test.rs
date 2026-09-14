//! Multi-tenant isolation: a user only sees / touches devices whose
//! pcname is in their assigned `devices` list.

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use serial_test::serial;
use wiremock::MockServer;

fn two_devices_json() -> String {
    serde_json::json!({
        "version": 1,
        "devices": [
            {"pctype": "macos", "pcname": "dev-one", "public_url": "http://127.0.0.1:1", "oc_serve_port": 4040},
            {"pctype": "windows", "pcname": "dev-two", "public_url": "http://127.0.0.1:2", "oc_serve_port": 4040},
        ]
    })
    .to_string()
}

#[tokio::test]
#[serial]
async fn user_only_sees_their_assigned_devices() {
    let sb = MockServer::start().await;
    sb_get(&sb, &devices_fs_path(), 200, &two_devices_json()).await;
    let app = test_app(build_state(&sb.uri()));

    // The user is assigned only dev-one.
    let cookie = login_as(&app, &sb, &["dev-one"]).await;
    let resp = send(&app, authed_get("/api/devices", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let devices = body_json(resp).await.as_array().unwrap().clone();
    assert_eq!(devices.len(), 1, "only assigned devices are listed");
    assert_eq!(devices[0]["pcname"], "dev-one");
}

#[tokio::test]
#[serial]
async fn foreign_device_projects_returns_403_forbidden() {
    let sb = MockServer::start().await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = login_as(&app, &sb, &["dev-one"]).await;

    let resp = send(
        &app,
        authed_get("/api/devices/windows/dev-two/projects", &cookie),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "forbidden");
}

#[tokio::test]
#[serial]
async fn foreign_device_delete_returns_403_forbidden() {
    let sb = MockServer::start().await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = login_as(&app, &sb, &["dev-one"]).await;

    let resp = send(
        &app,
        json_req(
            Method::DELETE,
            "/api/devices/windows/dev-two",
            &serde_json::json!({}),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
#[serial]
async fn assigned_device_delete_still_works() {
    let sb = MockServer::start().await;
    sb_get(&sb, &devices_fs_path(), 200, &two_devices_json()).await;
    sb_put_204(&sb, &devices_fs_path()).await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = login_as(&app, &sb, &["dev-one"]).await;

    let resp = send(
        &app,
        json_req(
            Method::DELETE,
            "/api/devices/macos/dev-one",
            &serde_json::json!({}),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK, "own device remains operable");
}

#[tokio::test]
#[serial]
async fn seed_requires_device_in_user_list() {
    let sb = MockServer::start().await;
    sb_put_204(&sb, &devices_fs_path()).await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = login_as(&app, &sb, &["dev-one"]).await;

    // Seeding an unassigned pcname → 403 (it would be invisible anyway).
    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/devices/seed",
            &serde_json::json!({
                "pcname": "dev-other",
                "public_url": "http://127.0.0.1:9999",
                "pctype": "windows",
            }),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
#[serial]
async fn deleted_user_session_becomes_unauthorized() {
    let sb = MockServer::start().await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = login_as(&app, &sb, &["dev-one"]).await;

    // Sanity: works while the user exists.
    let resp = send(&app, authed_get("/api/me", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // "Delete" the user by serving an empty registry on the next load.
    // The UsersCache TTL (30s) would normally hold the old value, so
    // invalidate by pointing a fresh state at a registry without the user.
    let sb2 = MockServer::start().await;
    sb_get_404(&sb2, users_fs_path()).await;
    let app2 = test_app(build_state(&sb2.uri()));
    // The cookie is signed with the same deterministic test key, so it is
    // valid for app2 too — but the user no longer resolves there.
    let resp = send(&app2, authed_get("/api/me", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
