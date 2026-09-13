//! Shared helpers for the BFF integration tests.
//!
//! Builds a real `AppState` by hand (no `AppState::from_env`, no env vars,
//! no filesystem cookie key, no live SilverBullet login) and pairs it with
//! wiremock stand-ins:
//! - one mock server plays SilverBullet (`/.fs/*` documents — devices.json
//!   plus the multi-tenant `web/opencode/config.md` user registry),
//! - per-test mock servers play the `oc serve` device endpoints
//!   (`/health`, `/project`, `/session`, `/api/session`).
//!
//! Device URLs are not part of `AppState`: devices are discovered from
//! `devices.json` served by the SB mock, so each test embeds its device
//! mock URLs there via `one_device_json` / raw `json!` documents.

#![allow(dead_code)]

use axum::body::Body;
use axum::http::{header, Method, Request, Response, StatusCode};
use axum::Router;
use mini_oc_web::auth::RateLimiter;
use mini_oc_web::devices::DevicesCache;
use mini_oc_web::sb::SbClient;
use mini_oc_web::state::{AppConfig, AppState};
use mini_oc_web::users::UsersCache;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Portal user name baked into the test users registry.
pub const TEST_USER: &str = "tester";
/// Unified device-hop credentials baked into the test `AppConfig`
/// (OPENCODE_SERVER_USERNAME/PASSWORD equivalent — NOT portal login).
pub const TEST_PASS: &str = "portal-secret";
/// 32-char login key of the default test user (letters/digits, no symbols).
pub const TEST_KEY: &str = "k1234567890123456789012345678901";
/// Stable 6-digit id of the default test user.
pub const TEST_USER_ID: &str = "100001";
/// SilverBullet password — doubles as the admin (SB password) login.
pub const SB_PASS: &str = "sb-secret";
/// SilverBullet user — selects the `serv/opencode/{user}/devices.json` path.
pub const SB_USER: &str = "admin";
/// Session cookie name (mirrors the private const in `src/routes.rs`).
pub const COOKIE_NAME: &str = "mini_oc_web_session";
/// Admin cookie name (mirrors the private const in `src/routes.rs`).
pub const ADMIN_COOKIE_NAME: &str = "mini_oc_admin";
/// Device names the default test user may see — covers every pcname the
/// integration suites register, so plain [`login`] just works. Tests that
/// need isolation use [`login_as`].
pub const DEFAULT_DEVICES: &[&str] = &[
    "dev-one",
    "dev-online",
    "dev-offline",
    "dev-seed",
    "auto-reg",
    "ghost",
    "samuel",
    "HomeWin",
];

/// SB path (without `/.fs/` prefix) of the devices list for [`SB_USER`].
pub fn devices_fs_path() -> String {
    format!("serv/opencode/{}/devices.json", SB_USER)
}

/// SB path of the multi-tenant user registry.
pub fn users_fs_path() -> &'static str {
    "web/opencode/config.md"
}

/// Deterministic 32-byte HMAC cookie key.
pub fn cookie_key() -> Vec<u8> {
    (0u8..32).collect()
}

/// The multi-tenant user registry document with a single default user
/// (`tester`, key [`TEST_KEY`]) whose device list is `devices` (port 4040
/// for every entry — tests only care about the names for isolation; the
/// desc mirrors the service name, pctype defaults to windows).
pub fn users_json(devices: &[&str]) -> String {
    serde_json::json!({
        "version": 1,
        "users": [{
            "id": TEST_USER_ID,
            "name": TEST_USER,
            "key": TEST_KEY,
            "devices": devices
                .iter()
                .map(|d| serde_json::json!({"desc": d, "name": d, "port": 4040, "pctype": "windows", "bound": false}))
                .collect::<Vec<_>>(),
            "created_at": "2026-09-13T00:00:00+08:00",
            "updated_at": "2026-09-13T00:00:00+08:00"
        }]
    })
    .to_string()
}

/// Hand-assemble an `AppState` around a wiremock SilverBullet base URL.
///
/// Mirrors what `AppState::from_env` produces, minus every side effect:
/// no dotenv, no key file, no eager `sb.login()` (the SB mock serves
/// `/.fs/*` without demanding cookies, and only 401s would trigger the
/// client's auto-relogin).
pub fn build_state(sb_base_url: &str) -> Arc<AppState> {
    let sb = Arc::new(SbClient::new(sb_base_url, SB_USER, SB_PASS).unwrap());
    let devices = Arc::new(DevicesCache::new(
        sb.clone(),
        SB_USER,
        Duration::from_secs(30),
    ));
    let users = Arc::new(UsersCache::new(sb.clone(), Duration::from_secs(30)));
    let rate_limiter = Arc::new(RateLimiter::new(5, Duration::from_secs(600)));
    Arc::new(AppState {
        config: AppConfig {
            web_port: 8100,
            web_bind: "127.0.0.1".to_string(),
            web_static_dir: "./web/dist".to_string(),
            device_user: TEST_USER.to_string(),
            device_pass: TEST_PASS.to_string(),
            sb_user: SB_USER.to_string(),
            sb_base_url: sb_base_url.to_string(),
            sb_password: SB_PASS.to_string(),
            rathole_key: format!("{SB_PASS}-rathole"),
            cookie_key: cookie_key(),
            portal_base: "https://oc.example.com".to_string(),
        },
        sb,
        devices,
        users,
        rate_limiter,
        device_creds: Arc::new(mini_oc_web::state::DeviceCreds::new()),
        started_at: Instant::now(),
    })
}

/// Build the full application router (all routes + SPA fallback) for
/// `tower::ServiceExt::oneshot` requests — no real port is bound.
pub fn test_app(state: Arc<AppState>) -> Router {
    mini_oc_web::routes::build_router((*state).clone())
}

/// Body-less request (GET/HEAD or empty POST).
pub fn req(method: Method, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

/// JSON request with an optional session cookie pair (`name=value`).
pub fn json_req(
    method: Method,
    uri: &str,
    body: &serde_json::Value,
    cookie: Option<&str>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(c) = cookie {
        builder = builder.header(header::COOKIE, c);
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

/// Cookie-authenticated body-less GET.
pub fn authed_get(uri: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .unwrap()
}

/// Drive the router with a single in-memory request.
pub async fn send(app: &Router, request: Request<Body>) -> Response<Body> {
    app.clone().oneshot(request).await.unwrap()
}

/// Collect the response body as raw bytes.
pub async fn body_bytes(resp: Response<Body>) -> Vec<u8> {
    axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec()
}

/// Collect the response body as a UTF-8 string.
pub async fn body_string(resp: Response<Body>) -> String {
    String::from_utf8(body_bytes(resp).await).unwrap()
}

/// Collect the response body as parsed JSON.
pub async fn body_json(resp: Response<Body>) -> serde_json::Value {
    serde_json::from_slice(&body_bytes(resp).await).unwrap()
}

/// Mount the users registry on the SB mock, then log the default user in
/// via `POST /api/login {key}` and return the `name=value` cookie pair.
pub async fn login_as(app: &Router, sb: &MockServer, devices: &[&str]) -> String {
    sb_get(sb, users_fs_path(), 200, &users_json(devices)).await;
    let resp = send(
        app,
        json_req(
            Method::POST,
            "/api/login",
            &serde_json::json!({"key": TEST_KEY}),
            None,
        ),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "login helper must succeed, got {}",
        resp.status()
    );
    let set_cookie = resp
        .headers()
        .get(header::SET_COOKIE)
        .expect("login must set a session cookie")
        .to_str()
        .unwrap()
        .to_string();
    set_cookie.split(';').next().unwrap().trim().to_string()
}

/// [`login_as`] with the suite-wide [`DEFAULT_DEVICES`] list.
pub async fn login(app: &Router, sb: &MockServer) -> String {
    login_as(app, sb, DEFAULT_DEVICES).await
}

/// Log the super-admin in with the SB password; return the admin cookie
/// pair for `/api/admin/*` requests.
pub async fn admin_login(app: &Router) -> String {
    let resp = send(
        app,
        json_req(
            Method::POST,
            "/api/admin/login",
            &serde_json::json!({"password": SB_PASS}),
            None,
        ),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "admin login helper must succeed, got {}",
        resp.status()
    );
    let set_cookie = resp
        .headers()
        .get(header::SET_COOKIE)
        .expect("admin login must set an admin cookie")
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        set_cookie.starts_with(&format!("{ADMIN_COOKIE_NAME}=")),
        "admin cookie name wrong: {set_cookie}"
    );
    set_cookie.split(';').next().unwrap().trim().to_string()
}

/// URL-safe base64 (no padding) — mirrors `mini_oc_web::jump::pcname_b64`
/// and the directory encoding inside `build_jump_url`.
pub fn b64url(s: &str) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    URL_SAFE_NO_PAD.encode(s.as_bytes())
}

/// Mount a SilverBullet `GET /.fs/<fs_path>` mock responding `status` + text body.
pub async fn sb_get(server: &MockServer, fs_path: &str, status: u16, body: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/.fs/{}", fs_path)))
        .respond_with(ResponseTemplate::new(status).set_body_string(body.to_string()))
        .mount(server)
        .await;
}

/// Mount a SilverBullet `GET /.fs/<fs_path>` → 404 (missing document).
pub async fn sb_get_404(server: &MockServer, fs_path: &str) {
    sb_get(server, fs_path, 404, "").await;
}

/// Mount a SilverBullet `PUT /.fs/<fs_path>` mock responding 204.
pub async fn sb_put_204(server: &MockServer, fs_path: &str) {
    Mock::given(method("PUT"))
        .and(path(format!("/.fs/{}", fs_path)))
        .respond_with(ResponseTemplate::new(204))
        .mount(server)
        .await;
}

/// A `devices.json` document with a single device pointing at `public_url`.
pub fn one_device_json(pctype: &str, pcname: &str, public_url: &str) -> String {
    serde_json::json!({
        "version": 1,
        "devices": [{
            "pctype": pctype,
            "pcname": pcname,
            "public_url": public_url,
            "oc_serve_port": 4040,
            "version": "1.0.0",
        }]
    })
    .to_string()
}
