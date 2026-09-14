//! `POST /api/device-bind` — 公开独立的设备绑定接口测试：
//! 以用户登录密钥认证，按 用户 id + 设备服务名 修改绑定状态并写回
//! 远程注册表 `web/opencode/config.md`。

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use serial_test::serial;
use wiremock::MockServer;

/// 正常路径：合法 key + 匹配 user_id + 清单内设备 → 200，返回更新后
/// 的设备，且远程注册表被 PUT 改写（对应设备 bound 变为 true）。
#[tokio::test]
#[serial]
async fn device_bind_updates_bound_and_writes_registry() {
    let sb = MockServer::start().await;
    sb_get(&sb, users_fs_path(), 200, &users_json(DEFAULT_DEVICES)).await;
    sb_put_204(&sb, users_fs_path()).await;
    let app = test_app(build_state(&sb.uri()));

    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/device-bind",
            &serde_json::json!({
                "key": TEST_KEY,
                "user_id": TEST_USER_ID,
                "name": "dev-one",
                "device-name": "HOME-OFFICE-PC",
                "bound": true,
            }),
            None,
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["ok"], true);
    assert_eq!(body["user_id"], TEST_USER_ID);
    assert_eq!(body["device"]["name"], "dev-one");
    assert_eq!(body["device"]["bound"], true);
    assert_eq!(body["device"]["device-name"], "HOME-OFFICE-PC");

    // The registry was PUT back with the new binding status AND the
    // client-reported device name.
    let puts: Vec<_> = sb
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.method.as_str() == "PUT" && r.url.path() == "/.fs/web/opencode/config.md")
        .collect();
    assert_eq!(puts.len(), 1, "exactly one registry write");
    let written: serde_json::Value = serde_json::from_slice(&puts[0].body).unwrap();
    let devices = written["users"][0]["devices"].as_array().unwrap();
    let dev_one = devices
        .iter()
        .find(|d| d["name"] == "dev-one")
        .expect("dev-one kept in the registry");
    assert_eq!(dev_one["bound"], true);
    assert_eq!(dev_one["device-name"], "HOME-OFFICE-PC");
    // Sibling devices are untouched (still unbound, no device name).
    let dev_two = devices.iter().find(|d| d["name"] == "dev-online").unwrap();
    assert_eq!(dev_two["bound"], false);
    assert_eq!(dev_two["device-name"], "");
}

/// 解绑回写：bound=false 同样生效。
#[tokio::test]
#[serial]
async fn device_bind_supports_unbinding() {
    let sb = MockServer::start().await;
    let mut registry: serde_json::Value =
        serde_json::from_str(&users_json(DEFAULT_DEVICES)).unwrap();
    registry["users"][0]["devices"][0]["bound"] = true.into();
    sb_get(&sb, users_fs_path(), 200, &registry.to_string()).await;
    sb_put_204(&sb, users_fs_path()).await;
    let app = test_app(build_state(&sb.uri()));

    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/device-bind",
            &serde_json::json!({
                "key": TEST_KEY,
                "user_id": TEST_USER_ID,
                "name": registry["users"][0]["devices"][0]["name"].as_str().unwrap(),
                "device-name": "HOME-OFFICE-PC",
                "bound": false,
            }),
            None,
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["device"]["bound"], false);
}

/// 错误密钥 → 401，不产生任何注册表写入。
#[tokio::test]
#[serial]
async fn device_bind_rejects_bad_key() {
    let sb = MockServer::start().await;
    sb_get(&sb, users_fs_path(), 200, &users_json(DEFAULT_DEVICES)).await;
    let app = test_app(build_state(&sb.uri()));

    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/device-bind",
            &serde_json::json!({
                "key": "wrong-key-wrong-key-wrong-key-wr",
                "user_id": TEST_USER_ID,
                "name": "dev-one",
                "device-name": "EVIL-PC",
                "bound": true,
            }),
            None,
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let puts: Vec<_> = sb
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.method.as_str() == "PUT" && r.url.path() == "/.fs/web/opencode/config.md")
        .collect();
    assert_eq!(puts.len(), 0, "bad key must not write the registry");
}

/// 密钥有效但 user_id 不属于该密钥 → 403（密钥只能操作所属用户）。
#[tokio::test]
#[serial]
async fn device_bind_rejects_user_id_mismatch() {
    let sb = MockServer::start().await;
    sb_get(&sb, users_fs_path(), 200, &users_json(DEFAULT_DEVICES)).await;
    let app = test_app(build_state(&sb.uri()));

    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/device-bind",
            &serde_json::json!({
                "key": TEST_KEY,
                "user_id": "999999",
                "name": "dev-one",
                "device-name": "HOME-OFFICE-PC",
                "bound": true,
            }),
            None,
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// 设备不在该用户清单中 → 404。
#[tokio::test]
#[serial]
async fn device_bind_unknown_device_returns_404() {
    let sb = MockServer::start().await;
    sb_get(&sb, users_fs_path(), 200, &users_json(DEFAULT_DEVICES)).await;
    let app = test_app(build_state(&sb.uri()));

    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/device-bind",
            &serde_json::json!({
                "key": TEST_KEY,
                "user_id": TEST_USER_ID,
                "name": "not-in-list",
                "device-name": "HOME-OFFICE-PC",
                "bound": true,
            }),
            None,
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// device-name 必传且限长：缺失 / 空白 / 超长 / 控制字符 / URL 结构
/// 字符（作为 SB 路径段会破坏层级）→ 400，不写注册表。
#[tokio::test]
#[serial]
async fn device_bind_requires_valid_device_name() {
    let sb = MockServer::start().await;
    sb_get(&sb, users_fs_path(), 200, &users_json(DEFAULT_DEVICES)).await;
    let app = test_app(build_state(&sb.uri()));

    // Missing field → 422 (axum Json rejection: required field absent).
    let resp = send(
        &app,
        json_req(
            Method::POST,
            "/api/device-bind",
            &serde_json::json!({
                "key": TEST_KEY,
                "user_id": TEST_USER_ID,
                "name": "dev-one",
                "bound": true,
            }),
            None,
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY, "missing device-name");

    for bad in ["", "   ", &"x".repeat(65), "bad\nname", "a/b", "a?b", "a#b", "a%b", "a\\b"] {
        let resp = send(
            &app,
            json_req(
                Method::POST,
                "/api/device-bind",
                &serde_json::json!({
                    "key": TEST_KEY,
                    "user_id": TEST_USER_ID,
                    "name": "dev-one",
                    "device-name": bad,
                    "bound": true,
                }),
                None,
            ),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "device-name {bad:?} must be rejected"
        );
    }

    let puts: Vec<_> = sb
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.method.as_str() == "PUT" && r.url.path() == "/.fs/web/opencode/config.md")
        .collect();
    assert_eq!(puts.len(), 0, "invalid device-name must not write the registry");
}
