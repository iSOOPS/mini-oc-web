//! UsersCache (the multi-tenant registry at `web/opencode/config.md`)
//! integration tests, plus validation-surface checks that need the wiremock
//! SB stand-in.

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use mini_oc_web::users::{validate_user_name, UserDevice, UsersCache, USERS_PATH};
use serial_test::serial;
use std::sync::Arc;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
#[serial]
async fn users_cache_returns_empty_on_missing_document() {
    let sb = MockServer::start().await;
    sb_get_404(&sb, USERS_PATH).await;
    let cache = UsersCache::new(
        Arc::new(mini_oc_web::sb::SbClient::new(sb.uri(), SB_USER, SB_PASS).unwrap()),
        Duration::from_secs(30),
    );

    let uf = cache.load().await.unwrap();
    assert_eq!(uf.version, 1);
    assert!(uf.users.is_empty());
}

#[tokio::test]
#[serial]
async fn users_cache_load_parses_document() {
    let sb = MockServer::start().await;
    sb_get(&sb, USERS_PATH, 200, &users_json(DEFAULT_DEVICES)).await;
    let cache = UsersCache::new(
        Arc::new(mini_oc_web::sb::SbClient::new(sb.uri(), SB_USER, SB_PASS).unwrap()),
        Duration::from_secs(30),
    );

    let uf = cache.load().await.unwrap();
    assert_eq!(uf.users.len(), 1);
    assert_eq!(uf.users[0].name, TEST_USER);
    assert_eq!(uf.users[0].key, TEST_KEY);
    let expected: Vec<UserDevice> = DEFAULT_DEVICES
        .iter()
        .map(|d| UserDevice {
            desc: d.to_string(),
            name: d.to_string(),
            port: 4040,
            device_name: String::new(),
            pctype: "windows".into(),
            bound: false,
        })
        .collect();
    assert_eq!(uf.users[0].devices, expected);
}

#[tokio::test]
#[serial]
async fn users_cache_save_writes_and_invalidates() {
    let sb = MockServer::start().await;
    // First load: registry with the default user. save() must PUT the
    // mutated file and invalidate so the next load re-reads from SB —
    // here the mock keeps serving the ORIGINAL document, which proves the
    // cache was actually dropped (otherwise we'd see the mutated copy).
    sb_get(&sb, USERS_PATH, 200, &users_json(DEFAULT_DEVICES)).await;
    sb_put_204(&sb, USERS_PATH).await;
    let cache = UsersCache::new(
        Arc::new(mini_oc_web::sb::SbClient::new(sb.uri(), SB_USER, SB_PASS).unwrap()),
        Duration::from_secs(30),
    );

    let mut uf = cache.load().await.unwrap();
    uf.users.clear();
    cache.save(&uf).await.unwrap();

    let reloaded = cache.load().await.unwrap();
    assert_eq!(
        reloaded.users.len(),
        1,
        "save must invalidate the cache — the next load re-reads SB"
    );

    // Exactly one PUT with an empty users array.
    let puts: Vec<_> = sb
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.method.as_str() == "PUT" && r.url.path() == format!("/.fs/{}", USERS_PATH))
        .collect();
    assert_eq!(puts.len(), 1);
    let written: serde_json::Value = serde_json::from_slice(&puts[0].body).unwrap();
    assert_eq!(written["users"].as_array().unwrap().len(), 0);
}

#[tokio::test]
#[serial]
async fn users_cache_load_fresh_bypasses_ttl_cache() {
    let sb = MockServer::start().await;
    // Expect exactly 2 remote GETs: the first `load`, then `load_fresh`
    // (the second plain `load` must be served from the warm TTL cache).
    // Counting requests sidesteps wiremock's first-mounted-wins matching.
    let _get_mock = Mock::given(method("GET"))
        .and(path(format!("/.fs/{}", USERS_PATH)))
        .respond_with(ResponseTemplate::new(200).set_body_string(users_json(DEFAULT_DEVICES)))
        .expect(2)
        .mount_as_scoped(&sb)
        .await;
    let cache = UsersCache::new(
        Arc::new(mini_oc_web::sb::SbClient::new(sb.uri(), SB_USER, SB_PASS).unwrap()),
        Duration::from_secs(30),
    );

    let uf = cache.load().await.unwrap();
    assert_eq!(uf.users.len(), 1, "first load reads the remote registry");

    let cached = cache.load().await.unwrap();
    assert_eq!(cached.users.len(), 1, "second load within the TTL window");

    let fresh = cache.load_fresh().await.unwrap();
    assert_eq!(
        fresh.users.len(),
        1,
        "load_fresh re-reads the remote document (not the warm cache)"
    );

    // The scoped mock's expectations (exactly 2 remote GETs) are checked
    // when `get_mock` drops at the end of this test.
}

/// Loading a registry whose users still carry legacy uuid ids rewrites
/// them to unique 6-digit ids and persists the migrated document back
/// to SB before serving it.
#[tokio::test]
#[serial]
async fn legacy_uuid_registry_migrates_to_6_digit_ids() {
    let sb = MockServer::start().await;
    sb_get(
        &sb,
        USERS_PATH,
        200,
        &serde_json::json!({
            "version": 1,
            "users": [
                {
                    "id": "11111111-1111-4111-8111-111111111111",
                    "name": "legacy-one",
                    "key": "k1234567890123456789012345678901",
                    "devices": [],
                },
                {
                    "id": TEST_USER_ID,
                    "name": TEST_USER,
                    "key": TEST_KEY,
                    "devices": [],
                },
            ]
        })
        .to_string(),
    )
    .await;
    sb_put_204(&sb, USERS_PATH).await;
    let cache = UsersCache::new(
        Arc::new(mini_oc_web::sb::SbClient::new(sb.uri(), SB_USER, SB_PASS).unwrap()),
        Duration::from_secs(30),
    );

    let uf = cache.load().await.unwrap();
    assert_eq!(
        uf.users[1].id, TEST_USER_ID,
        "already-6-digit id untouched by migration"
    );
    let migrated = &uf.users[0].id;
    assert_eq!(migrated.len(), 6, "uuid rewritten to 6 digits: {migrated}");
    assert!(
        migrated.chars().all(|c| c.is_ascii_digit()),
        "digits only: {migrated}"
    );
    assert_ne!(migrated, TEST_USER_ID, "migrated id stays unique");

    // The migrated document was persisted to SB before being served.
    let puts: Vec<_> = sb
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.method.as_str() == "PUT" && r.url.path() == format!("/.fs/{}", USERS_PATH))
        .collect();
    assert_eq!(puts.len(), 1, "migration writes the registry back once");
    let written: serde_json::Value = serde_json::from_slice(&puts[0].body).unwrap();
    assert_eq!(written["users"][0]["id"], *migrated);
    assert_eq!(written["users"][1]["id"], TEST_USER_ID);
}

#[test]
fn user_name_follows_pcname_whitelist() {
    assert!(validate_user_name("samuel").is_ok());
    assert!(validate_user_name(&"a".repeat(64)).is_ok());
    assert!(validate_user_name("").is_err());
    assert!(validate_user_name("bad name").is_err());
    assert!(validate_user_name(&"a".repeat(65)).is_err());
}

#[tokio::test]
#[serial]
async fn login_via_cache_serves_second_login_without_extra_sb_hits() {
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

    let gets: Vec<_> = sb
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.method.as_str() == "GET" && r.url.path() == format!("/.fs/{}", USERS_PATH))
        .collect();
    assert_eq!(gets.len(), 1, "login reads the registry through the cache");
}

/// Regression: registries written before the 设备清单 refactor carried
/// bare-string `devices` and a top-level `oc_serve_port`. They must keep
/// loading (legacy entries default to port 4040) instead of 500-ing
/// every login with "users config parse: invalid type: string".
#[tokio::test]
#[serial]
async fn legacy_string_devices_registry_still_logs_in() {
    let sb = MockServer::start().await;
    let legacy = serde_json::json!({
        "version": 1,
        "users": [{
            "id": TEST_USER_ID,
            "name": TEST_USER,
            "key": TEST_KEY,
            "oc_serve_port": 4040,
            "devices": ["home-win"],
            "created_at": "2026-09-13T00:00:00+08:00",
            "updated_at": "2026-09-13T00:00:00+08:00"
        }]
    })
    .to_string();
    sb_get(&sb, users_fs_path(), 200, &legacy).await;
    sb_get_404(&sb, &devices_fs_path()).await;
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
    assert_eq!(resp.status(), StatusCode::OK, "legacy registry must log in");

    // The legacy entry surfaces through /api/me as a structured device.
    let cookie = resp
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .trim()
        .to_string();
    let resp = send(&app, authed_get("/api/me", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["devices"][0]["name"], "home-win");
    assert_eq!(body["devices"][0]["port"], 4040);
}

