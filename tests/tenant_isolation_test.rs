//! Multi-tenant isolation: a user only sees / touches devices whose
//! `name` is in their assigned `devices` list.

mod common;

use axum::http::StatusCode;
use common::*;
use serial_test::serial;
use wiremock::MockServer;

#[tokio::test]
#[serial]
async fn user_only_sees_their_assigned_devices() {
    let sb = MockServer::start().await;
    let app = test_app(build_state(&sb.uri()));

    // The user is assigned only dev-one — /api/me's enriched `devices`
    // list must reflect that.
    let cookie = login_as(&app, &sb, &["dev-one"]).await;
    let resp = send(&app, authed_get("/api/me", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let names: Vec<&str> = body["devices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["dev-one"], "only assigned devices appear in /api/me");
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
async fn deleted_user_session_becomes_unauthorized() {
    let sb = MockServer::start().await;
    let app = test_app(build_state(&sb.uri()));
    let cookie = login_as(&app, &sb, &["dev-one"]).await;

    // Sanity: works while the user exists.
    let resp = send(&app, authed_get("/api/me", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // "Delete" the user by serving an empty registry on the next load.
    // There is no cache anymore — every load() reads SB directly — so
    // pointing a fresh state at a registry without the user suffices.
    let sb2 = MockServer::start().await;
    sb_get_404(&sb2, users_fs_path()).await;
    let app2 = test_app(build_state(&sb2.uri()));
    // The cookie is signed with the same deterministic test key, so it is
    // valid for app2 too — but the user no longer resolves there.
    let resp = send(&app2, authed_get("/api/me", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
