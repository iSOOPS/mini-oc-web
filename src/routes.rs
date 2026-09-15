use crate::auth::{sign_cookie, verify_cookie};
use crate::devices::{
    path_list_path, probe_device_status, validate_pcname, Device, DevicesFile, PathListEntry,
    DEVICES_PATH,
};
use crate::error::{AppError, AppResult};
use crate::jump::{
    append_auth_token, build_jump_url, pcname_b64, resolve_base_url, validate_base_url, JumpInput,
};
use crate::proxy::{CreatedSession, DeviceClient, ProjectInfo, SessionInfo};
use crate::state::AppState;
use crate::users::{
    generate_key, generate_user_id_unique, validate_cloud_ip, validate_sb_config,
    validate_user_devices, validate_user_name, PortalUser, UserDevice, UserSbConfig, UsersFile,
};
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json as AxumJson, Router};
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const COOKIE_NAME: &str = "mini_oc_web_session";
const COOKIE_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
/// Separate cookie for the super-admin session (login = SB password).
const ADMIN_COOKIE_NAME: &str = "mini_oc_admin";
/// Admin sessions are shorter-lived than user sessions (higher privilege).
const ADMIN_COOKIE_TTL: Duration = Duration::from_secs(12 * 3600);
/// Cookie subject for the admin session.
const ADMIN_SUBJECT: &str = "admin";

/// Build the full Axum router.
///
/// The BFF serves the SPA on every non-`/api`, non-`/healthz` route via a
/// static-file handler. Unknown paths fall back to `index.html` so
/// `vue-router` history mode works on refresh.
pub fn build_router(state: AppState) -> Router {
    let shared = Arc::new(state);
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/me/key", get(me_key))
        .route("/api/me/name", post(me_rename))
        .route("/api/me/cloud-ip", post(me_cloud_ip))
        .route("/api/me/sb", post(me_sb))
        .route("/api/user/info", post(user_info_by_key))
        .route("/api/device-bind", post(device_bind))
        .route("/api/device-status/:port", get(device_status))
        .route("/api/devices", get(devices))
        .route("/api/devices/seed", post(devices_seed))
        .route(
            "/api/devices/:pctype/:pcname/auth-check",
            get(device_auth_check),
        )
        .route("/api/devices/:pctype/:pcname/auth", post(device_auth))
        .route(
            "/api/devices/:pctype/:pcname/projects",
            get(device_projects),
        )
        .route(
            "/api/devices/:pctype/:pcname/sessions",
            get(device_sessions).post(device_create_session),
        )
        .route(
            "/api/devices/:pctype/:pcname/jump",
            get(device_jump),
        )
        .route(
            "/api/devices/:pctype/:pcname",
            axum::routing::delete(device_delete),
        )
        .route(
            "/api/devices/:pctype/:pcname/detail",
            get(device_detail),
        )
        .route("/api/admin/login", post(admin_login))
        .route("/api/admin/logout", post(admin_logout))
        .route("/api/admin/info", get(admin_info))
        .route(
            "/api/admin/users",
            get(admin_users_list).post(admin_users_create),
        )
        .route(
            "/api/admin/users/:id",
            axum::routing::put(admin_users_update).delete(admin_users_delete),
        )
        .route(
            "/api/admin/users/:id/regenerate-key",
            post(admin_users_regenerate_key),
        )
        .fallback(spa_handler)
        .with_state(shared)
}

/// Serve static files from `WEB_STATIC_DIR` with SPA-style `index.html`
/// fallback. Unknown paths return `index.html` (with the right MIME type)
/// so `vue-router`'s history mode keeps working after a page refresh.
async fn spa_handler(State(state): State<Arc<AppState>>, uri: axum::http::Uri) -> Response {
    let dir = PathBuf::from(&state.config.web_static_dir);
    let uri_path = uri.path();
    let rel = uri_path.trim_start_matches('/');
    let candidate = if rel.is_empty() {
        dir.join("index.html")
    } else {
        dir.join(rel)
    };

    if candidate.exists() && candidate.is_file() {
        match tokio::fs::read(&candidate).await {
            Ok(bytes) => file_response(&bytes, &candidate),
            Err(_) => not_found(),
        }
    } else if candidate.exists() && candidate.is_dir() {
        let index = candidate.join("index.html");
        match tokio::fs::read(&index).await {
            Ok(bytes) => file_response(&bytes, &index),
            Err(_) => index_response(&dir).await,
        }
    } else {
        index_response(&dir).await
    }
}

fn file_response(bytes: &[u8], path: &std::path::Path) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.as_ref())
        .body(Body::from(bytes.to_vec()))
        .unwrap()
}

async fn index_response(dir: &std::path::Path) -> Response {
    let index = dir.join("index.html");
    match tokio::fs::read(&index).await {
        Ok(bytes) => file_response(&bytes, &index),
        Err(_) => not_found(),
    }
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        "404 not found\n",
    )
        .into_response()
}

fn extract_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .or_else(|| headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()))
        .and_then(|s| s.split(',').next())
        .unwrap_or("0.0.0.0")
        .to_string()
}

fn require_session(headers: &HeaderMap, key: &[u8]) -> AppResult<String> {
    let cookie = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .ok_or(AppError::Unauthorized("missing cookie".into()))?;
    let session = cookie
        .split(';')
        .map(|s| s.trim())
        .find_map(|kv| kv.strip_prefix(&format!("{}=", COOKIE_NAME)))
        .ok_or(AppError::Unauthorized("no session cookie".into()))?;
    verify_cookie(session, key)
}

fn set_session_cookie(subject: &str, key: &[u8]) -> String {
    let cookie = sign_cookie(subject, key, COOKIE_TTL).unwrap();
    format!(
        "{}={}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}",
        COOKIE_NAME,
        cookie,
        COOKIE_TTL.as_secs()
    )
}

/// Pull the admin subject out of the `mini_oc_admin` cookie. Any other
/// subject (e.g. a user session value) is rejected.
fn require_admin(headers: &HeaderMap, key: &[u8]) -> AppResult<()> {
    let cookie = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .ok_or(AppError::Unauthorized("missing cookie".into()))?;
    let session = cookie
        .split(';')
        .map(|s| s.trim())
        .find_map(|kv| kv.strip_prefix(&format!("{}=", ADMIN_COOKIE_NAME)))
        .ok_or(AppError::Unauthorized("no admin cookie".into()))?;
    let subject = verify_cookie(session, key)?;
    if subject == ADMIN_SUBJECT {
        Ok(())
    } else {
        Err(AppError::Unauthorized("not an admin session".into()))
    }
}

/// Resolve the signed-in portal user (cookie subject `u:{id}`) against the
/// current `web/opencode/config.md` registry. A deleted user fails with
/// 401 — the browser soft-redirects to /login.
async fn current_user(state: &AppState, headers: &HeaderMap) -> AppResult<PortalUser> {
    let subject = require_session(headers, &state.config.cookie_key)?;
    let id = subject
        .strip_prefix("u:")
        .ok_or(AppError::Unauthorized("not a user session".into()))?;
    let uf = state.users.load().await?;
    uf.users
        .into_iter()
        .find(|u| u.id == id)
        .ok_or(AppError::Unauthorized("user no longer exists".into()))
}

/// Multi-tenant isolation: a user may only touch devices whose pcname is
/// in their assigned device list (设备清单).
fn ensure_device_allowed(user: &PortalUser, pcname: &str) -> AppResult<()> {
    if user.devices.iter().any(|ud| ud.name == pcname) {
        Ok(())
    } else {
        Err(AppError::Forbidden(format!(
            "device {pcname} not assigned to user {}",
            user.name
        )))
    }
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok\n")
}

#[derive(Deserialize)]
struct LoginBody {
    /// 32-char per-user login key (no username any more).
    key: String,
}

async fn login(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumJson(body): AxumJson<LoginBody>,
) -> AppResult<impl IntoResponse> {
    let ip = extract_ip(&headers);
    state.rate_limiter.check(&ip).await?;

    let mut uf = state.users.load().await?;
    // Clone just the id out of the immutable borrow so the registry can
    // be mutated for the last_used_at stamp below.
    let found_id = uf
        .users
        .iter()
        .find(|u| u.key == body.key)
        .map(|u| u.id.clone());
    match found_id {
        Some(user_id) => {
            // Stamp last_used_at (best-effort — a failed write-back must
            // never block the login itself).
            if let Some(u) = uf.users.iter_mut().find(|u| u.id == user_id) {
                u.last_used_at = Some(chrono::Local::now().to_rfc3339());
            }
            if let Err(e) = state.users.save(&uf).await {
                tracing::warn!("login last_used_at write-back failed: {e}");
            }
            state.rate_limiter.reset(&ip).await;
            let subject = format!("u:{user_id}");
            let cookie = set_session_cookie(&subject, &state.config.cookie_key);
            let mut resp_headers = HeaderMap::new();
            resp_headers.insert(axum::http::header::SET_COOKIE, cookie.parse().unwrap());
            Ok((
                StatusCode::OK,
                resp_headers,
                AxumJson(serde_json::json!({"ok": true})),
            ))
        }
        None => {
            state.rate_limiter.record_failure(&ip).await;
            Err(AppError::Unauthorized("bad key".into()))
        }
    }
}

/// Who am I: the signed-in user's portal profile (no key material) with
/// every assigned device's runtime status probed concurrently via the
/// user's cloud tunnel (`http://{cloud_ip}:{port}/status`).
async fn me(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult<AxumJson<serde_json::Value>> {
    let user = current_user(&state, &headers).await?;
    let probes = user
        .devices
        .iter()
        .map(|d| probe_device_status(&user.cloud_ip, d.port))
        .collect::<Vec<_>>();
    let results = join_all(probes).await;

    let mut enriched: Vec<serde_json::Value> = Vec::with_capacity(user.devices.len());
    for (ud, pr) in user.devices.iter().zip(results.into_iter()) {
        let mut entry = serde_json::json!({
            "desc": ud.desc,
            "name": ud.name,
            "port": ud.port,
            "device-name": ud.device_name,
            "pctype": ud.pctype,
            "bound": ud.bound,
            "online": pr.online,
        });
        if let Some(r) = pr.reason {
            entry["reason"] = serde_json::Value::String(r);
        }
        if let Some(s) = pr.status {
            entry["status"] = s;
        }
        enriched.push(entry);
    }

    Ok(AxumJson(serde_json::json!({
        "id": user.id,
        "name": user.name,
        "cloud_ip": user.cloud_ip,
        "last_used_at": user.last_used_at,
        "devices": enriched,
        "sb": user.sb,
    })))
}

/// Reveal the user's own login key (the "用户密码" in the settings page).
async fn me_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult<AxumJson<serde_json::Value>> {
    let user = current_user(&state, &headers).await?;
    Ok(AxumJson(serde_json::json!({"key": user.key})))
}

#[derive(Deserialize)]
struct MeNameBody {
    name: String,
}

/// Self-service rename. The session cookie is id-based, so the user stays
/// logged in after renaming.
async fn me_rename(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumJson(body): AxumJson<MeNameBody>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let user = current_user(&state, &headers).await?;
    validate_user_name(&body.name)?;
    let mut uf = state.users.load().await?;
    if uf.users.iter().any(|u| u.id != user.id && u.name == body.name) {
        return Err(AppError::InvalidTarget(format!(
            "user name already exists: {}",
            body.name
        )));
    }
    let target = uf
        .users
        .iter_mut()
        .find(|u| u.id == user.id)
        .ok_or(AppError::NotFound("user".into()))?;
    target.name = body.name.clone();
    target.updated_at = Some(chrono::Local::now().to_rfc3339());
    state.users.save(&uf).await?;
    Ok(AxumJson(serde_json::json!({"ok": true, "name": body.name})))
}

#[derive(Deserialize)]
struct MeCloudIpBody {
    ip: String,
}

/// Self-service cloud IP update (云服务设置). The device list itself is
/// admin-controlled and deliberately NOT editable here.
async fn me_cloud_ip(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumJson(body): AxumJson<MeCloudIpBody>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let user = current_user(&state, &headers).await?;
    validate_cloud_ip(&body.ip)?;
    let ip = body.ip.trim().to_string();
    let mut uf = state.users.load().await?;
    let target = uf
        .users
        .iter_mut()
        .find(|u| u.id == user.id)
        .ok_or(AppError::NotFound("user".into()))?;
    target.cloud_ip = ip.clone();
    target.updated_at = Some(chrono::Local::now().to_rfc3339());
    state.users.save(&uf).await?;
    Ok(AxumJson(serde_json::json!({"ok": true, "cloud_ip": ip})))
}

#[derive(Deserialize)]
struct MeSbBody {
    base_url: String,
    username: String,
    password: String,
}

/// Self-service SB config update（SB 设置：域名/账号/密码）. Trimmed and
/// validated, then persisted into the user's registry entry.
async fn me_sb(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumJson(body): AxumJson<MeSbBody>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let user = current_user(&state, &headers).await?;
    let sb = UserSbConfig {
        base_url: body.base_url.trim().to_string(),
        username: body.username.trim().to_string(),
        password: body.password.trim().to_string(),
    };
    validate_sb_config(&sb)?;
    let mut uf = state.users.load().await?;
    let target = uf
        .users
        .iter_mut()
        .find(|u| u.id == user.id)
        .ok_or(AppError::NotFound("user".into()))?;
    target.sb = sb.clone();
    target.updated_at = Some(chrono::Local::now().to_rfc3339());
    state.users.save(&uf).await?;
    Ok(AxumJson(serde_json::json!({"ok": true, "sb": sb})))
}

/// Fetch the caller's own full profile by login key (the "用户密码") —
/// no session cookie needed, so devices/scripts can pull their user's
/// complete config (id, key, sb, devices, …). Rate-limited like login.
async fn user_info_by_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumJson(body): AxumJson<LoginBody>,
) -> AppResult<AxumJson<PortalUser>> {
    let ip = extract_ip(&headers);
    state.rate_limiter.check(&ip).await?;

    let uf = state.users.load().await?;
    match uf.users.iter().find(|u| u.key == body.key) {
        Some(user) => {
            state.rate_limiter.reset(&ip).await;
            Ok(AxumJson(user.clone()))
        }
        None => {
            state.rate_limiter.record_failure(&ip).await;
            Err(AppError::Unauthorized("bad key".into()))
        }
    }
}

/// 公开独立的「设备绑定」接口（供客户端调用，无 session cookie）：
/// 以 32 位用户登录密钥认证（与 POST /api/login 同一凭证），请求体
/// 携带用户 id + 设备服务名 + 目标绑定状态。key 定位用户后必须与
/// user_id 一致（密钥只能操作所属用户自己的设备），随后在其设备
/// 清单中按服务名找到设备并改写 `bound`，整体写回远程注册表
/// `web/opencode/config.md`。限流策略与 login 一致。
#[derive(Deserialize)]
struct DeviceBindBody {
    /// 32-char portal login key — authenticates the caller.
    key: String,
    /// Target user id; must match the user the key belongs to.
    user_id: String,
    /// Device service name whose binding status to update.
    name: String,
    /// 设备名称（必传）：绑定客户端上报的真实设备名，写入对应设备
    /// 条目的 `device-name` 字段（管理端不可编辑该字段）。
    #[serde(rename = "device-name")]
    device_name: String,
    /// New binding status (true = 绑定, false = 解绑).
    bound: bool,
}

async fn device_bind(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumJson(body): AxumJson<DeviceBindBody>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let ip = extract_ip(&headers);
    state.rate_limiter.check(&ip).await?;

    let device_name = body.device_name.trim().to_string();
    if device_name.is_empty()
        || device_name.chars().count() > 64
        || device_name.chars().any(char::is_control)
        || device_name.chars().any(|c| "/?#%\\".contains(c))
    {
        return Err(AppError::InvalidTarget(
            "device-name must be 1-64 chars, no control characters, no URL-structure characters (/ ? # % \\)".into(),
        ));
    }

    let mut uf = state.users.load().await?;
    let owner_id = match uf.users.iter().find(|u| u.key == body.key) {
        Some(u) => u.id.clone(),
        None => {
            state.rate_limiter.record_failure(&ip).await;
            return Err(AppError::Unauthorized("bad key".into()));
        }
    };
    state.rate_limiter.reset(&ip).await;

    // The key may only touch its own user's devices.
    if owner_id != body.user_id {
        return Err(AppError::Forbidden(format!(
            "user id mismatch: key belongs to {owner_id}"
        )));
    }

    let user = uf
        .users
        .iter_mut()
        .find(|u| u.id == body.user_id)
        .ok_or(AppError::NotFound("user".into()))?;
    let device = user.devices.iter_mut().find(|d| d.name == body.name).ok_or(
        AppError::NotFound(format!(
            "device {} not in user {} device list",
            body.name, body.user_id
        )),
    )?;
    device.bound = body.bound;
    device.device_name = device_name;
    let updated = device.clone();
    state.users.save(&uf).await?;

    Ok(AxumJson(serde_json::json!({
        "ok": true,
        "user_id": body.user_id,
        "device": {
            "desc": updated.desc,
            "name": updated.name,
            "port": updated.port,
            "device-name": updated.device_name,
            "pctype": updated.pctype,
            "bound": updated.bound,
        },
    })))
}

/// Probe a device's real state through the user's cloud tunnel:
/// `GET http://{cloud_ip}:{port}/status` on the mini-oc-gui end side
/// (unauthenticated snapshot: opencode_serve / rathole / system info).
/// The device has no CORS layer, so the BFF proxies the request
/// server-side; the port must belong to the caller's device list.
async fn device_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(port): Path<u16>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let user = current_user(&state, &headers).await?;
    if !user.devices.iter().any(|d| d.port == port) {
        return Err(AppError::Forbidden(format!(
            "port {port} is not in your device list"
        )));
    }
    let url = format!("http://{}:{}/status", user.cloud_ip, port);
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
        .map_err(|e| AppError::Internal(format!("probe client: {}", e)))?;
    match http.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            let status: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| AppError::Internal(format!("status body parse: {}", e)))?;
            Ok(AxumJson(
                serde_json::json!({"online": true, "status": status}),
            ))
        }
        Ok(resp) => Ok(AxumJson(serde_json::json!({
            "online": false,
            "reason": format!("device returned HTTP {}", resp.status()),
        }))),
        Err(e) => Ok(AxumJson(serde_json::json!({
            "online": false,
            "reason": e.to_string(),
        }))),
    }
}

async fn logout() -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    let expired = format!(
        "{}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0",
        COOKIE_NAME
    );
    headers.insert(
        axum::http::header::SET_COOKIE,
        expired.parse().unwrap(),
    );
    (StatusCode::OK, headers, AxumJson(serde_json::json!({"ok": true})))
}

#[derive(Serialize)]
struct DeviceView {
    pctype: String,
    pcname: String,
    /// URL-safe base64 of `pcname`. The first URL segment on the portal host,
    /// e.g. `oc.isoops.com/c2FtdWVs/...`.
    pcname_b64: String,
    /// The portal host (`oc.isoops.com`) — what the SPA / devices page uses
    /// to build `<a href>` jump URLs.
    portal_base: String,
    /// Devices.json's `public_url`. Used for the BFF-to-device API hop and
    /// as the jump base URL.
    public_url: String,
    online: bool,
    version: Option<String>,
}

/// Devices visible to the signed-in user: `devices.json` entries filtered
/// by the user's assigned device names (multi-tenant isolation).
async fn devices(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult<AxumJson<Vec<DeviceView>>> {
    let user = current_user(&state, &headers).await?;
    let df: DevicesFile = state.devices.load().await?;

    let portal_base = state.config.portal_base.clone();
    let mut out = Vec::new();
    for d in df.devices {
        if !user.devices.iter().any(|ud| ud.name == d.pcname) {
            continue;
        }
        // Health probing is credential-free by design (ADR #13): pass empty
        // strings instead of placeholder credentials.
        let client = DeviceClient::new(&d.public_url, "", "");
        let online = client.health().await.unwrap_or(false);
        out.push(DeviceView {
            portal_base: portal_base.clone(),
            pcname_b64: pcname_b64(&d.pcname),
            public_url: d.public_url.clone(),
            pctype: d.pctype.clone(),
            pcname: d.pcname.clone(),
            online,
            version: d.version,
        });
    }
    Ok(AxumJson(out))
}

#[derive(Deserialize)]
struct SeedBody {
    pctype: String,
    pcname: String,
    public_url: String,
}

/// Device connection details for the SPA "详情" dialog: identity, base
/// URL, and the credentials the BFF would use for the device hop
/// (cached user-supplied pair first, unified config pair as fallback).
/// The password is returned in clear text — the SPA masks it client-side
/// (`*` per char) until the user reveals it. Session-protected like
/// every other /api route.
async fn device_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((pctype, pcname)): Path<(String, String)>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let user = current_user(&state, &headers).await?;
    let (_, _) = parse_device_path(&pctype, &pcname)?;
    ensure_device_allowed(&user, &pcname)?;
    let df = state.devices.load().await?;
    let device = find_device(&df, &pctype, &pcname)?;
    let (username, password) = device_creds_for(&state, device);
    Ok(AxumJson(serde_json::json!({
        "pctype": device.pctype,
        "pcname": device.pcname,
        "public_url": device.public_url,
        "oc_serve_port": device.oc_serve_port,
        "version": device.version,
        "username": username,
        "password": password,
    })))
}

/// Remove a device from `devices.json` (read-modify-write). Used by the
/// SPA for manually-added (seeded) device cards; end-side registered
/// devices manage their own lifecycle.
async fn device_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((pctype, pcname)): Path<(String, String)>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let user = current_user(&state, &headers).await?;
    let (pctype, pcname) = parse_device_path(&pctype, &pcname)?;
    ensure_device_allowed(&user, &pcname)?;
    let mut df = state.devices.load().await?;
    let key = format!("{}/{}", pctype, pcname);
    let before = df.devices.len();
    df.devices.retain(|d| d.key() != key);
    if df.devices.len() == before {
        return Err(AppError::NotFound("device".into()));
    }
    let path = DEVICES_PATH.replace("{user}", &state.config.sb_user);
    let json = serde_json::to_string_pretty(&df)
        .map_err(|e| AppError::Internal(format!("devices serialize: {}", e)))?;
    state.sb.put_fs(&path, &json).await?;
    Ok(AxumJson(serde_json::json!({ "ok": true })))
}

/// Register a demo device into `devices.json` without a live end-side
/// (mini-oc-gui) reporter (spec §3.2). Local-dev helper: kept for offline
/// demos after the end-side registration ships, not recommended in
/// production. Replaces any existing entry with the same pctype/pcname.
async fn devices_seed(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumJson(body): AxumJson<SeedBody>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let user = current_user(&state, &headers).await?;
    let (pctype, pcname) = parse_device_path(&body.pctype, &body.pcname)?;
    // Multi-tenant: seeding a pcname the caller may not see would produce
    // an invisible device, so require it to be in the user's device list.
    ensure_device_allowed(&user, &pcname)?;
    let public_url = validate_base_url(&body.public_url)?;

    // Read-modify-write devices.json: replace an existing entry with the
    // same pctype/pcname, otherwise append (`load` returns an empty file
    // when devices.json does not exist yet).
    let mut df = state.devices.load().await?;
    let key = format!("{}/{}", pctype, pcname);
    df.devices.retain(|d| d.key() != key);
    df.devices.push(Device {
        pctype,
        pcname,
        public_url,
        oc_serve_port: 4040,
        reported_at: Some(chrono::Local::now().to_rfc3339()),
        version: None,
    });
    let path = DEVICES_PATH.replace("{user}", &state.config.sb_user);
    let json = serde_json::to_string_pretty(&df)
        .map_err(|e| AppError::Internal(format!("devices serialize: {}", e)))?;
    state.sb.put_fs(&path, &json).await?;
    Ok(AxumJson(serde_json::json!({"ok": true})))
}

fn parse_device_path(pctype: &str, pcname: &str) -> AppResult<(String, String)> {
    if pctype != "macos" && pctype != "windows" {
        return Err(AppError::InvalidTarget(format!("bad pctype: {}", pctype)));
    }
    validate_pcname(pcname)?;
    Ok((pctype.to_string(), pcname.to_string()))
}

/// Find a devices.json entry by (pctype, pcname). Shared by every
/// `/api/devices/:pctype/:pcname/...` handler.
fn find_device<'a>(
    df: &'a DevicesFile,
    pctype: &str,
    pcname: &str,
) -> AppResult<&'a Device> {
    df.devices
        .iter()
        .find(|d| d.pctype == pctype && d.pcname == pcname)
        .ok_or(AppError::NotFound("device".into()))
}

/// Build a [`DeviceClient`] with the best credentials we have for this
/// device: user-supplied credentials cached via `POST .../auth` first,
/// unified `OPENCODE_SERVER_USERNAME/PASSWORD` as fallback.
fn cached_device_client(state: &AppState, device: &Device) -> DeviceClient {
    let (user, pass) = device_creds_for(state, device);
    DeviceClient::new(&device.public_url, &user, &pass)
}

/// Credentials for this device: cached user-supplied pair first, unified
/// config pair as fallback. Same priority as [`cached_device_client`];
/// used both for the proxy hop and for embedding `auth_token` into jump
/// URLs so the browser lands on the device web UI already authorized.
fn device_creds_for(state: &AppState, device: &Device) -> (String, String) {
    state.device_creds.get(&device.key()).unwrap_or_else(|| {
        (
            state.config.device_user.clone(),
            state.config.device_pass.clone(),
        )
    })
}

#[derive(Deserialize)]
struct DeviceAuthBody {
    username: String,
    password: String,
}

/// Probe whether the BFF can currently talk to a device's protected API.
///
/// The SPA calls this when the user clicks a device card: `{required:
/// true}` means the cached/unified credentials got a 401 from the device
/// and the SPA should pop the credential dialog before navigating. Any
/// other failure (device unreachable, etc.) propagates as an error so
/// the SPA can surface it.
async fn device_auth_check(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((pctype, pcname)): Path<(String, String)>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let user = current_user(&state, &headers).await?;
    let (_, _) = parse_device_path(&pctype, &pcname)?;
    ensure_device_allowed(&user, &pcname)?;
    let df = state.devices.load().await?;
    let device = find_device(&df, &pctype, &pcname)?;
    let client = cached_device_client(&state, device);
    match client.projects().await {
        Ok(_) => Ok(AxumJson(serde_json::json!({ "required": false }))),
        Err(AppError::DeviceAuthFailed(_)) => {
            Ok(AxumJson(serde_json::json!({ "required": true })))
        }
        Err(e) => Err(e),
    }
}

/// Verify user-supplied credentials against a device and cache them for
/// the BFF's server-side device calls. A bad pair returns
/// `device_auth_failed` (502) so the SPA can keep the dialog open and
/// show the error; a good pair returns `{ok: true}` and the SPA proceeds
/// to the (now authorized) device pages.
async fn device_auth(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((pctype, pcname)): Path<(String, String)>,
    AxumJson(body): AxumJson<DeviceAuthBody>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let user = current_user(&state, &headers).await?;
    let (_, _) = parse_device_path(&pctype, &pcname)?;
    ensure_device_allowed(&user, &pcname)?;
    let df = state.devices.load().await?;
    let device = find_device(&df, &pctype, &pcname)?;
    let client = DeviceClient::new(&device.public_url, &body.username, &body.password);
    client.projects().await?;
    state
        .device_creds
        .set(&device.key(), &body.username, &body.password);
    Ok(AxumJson(serde_json::json!({ "ok": true })))
}

/// Load the device's `path-list.md` from SilverBullet. The document is
/// located per the multi-tenant layout
/// `serv/opencode/{user_id}/{pctype}/{device_name}/path-list.md`:
/// the platform type and device name come from the caller's own
/// device-list entry (设备清单) — `pctype` assigned by the admin,
/// `device-name` reported by the binding client — falling back to the
/// URL's pctype / the service name for legacy entries that predate
/// those fields. A missing document is an empty registry (the end-side
/// hasn't synced yet); only transport and parse failures surface as
/// errors.
async fn load_path_list(
    state: &AppState,
    user: &PortalUser,
    url_pctype: &str,
    pcname: &str,
) -> AppResult<Vec<PathListEntry>> {
    let dev = user.devices.iter().find(|d| d.name == pcname).ok_or_else(|| {
        AppError::Forbidden(format!(
            "device {} not assigned to user {}",
            pcname, user.name
        ))
    })?;
    let pctype = if dev.pctype.is_empty() {
        url_pctype
    } else {
        dev.pctype.as_str()
    };
    let device_name = if dev.device_name.is_empty() {
        pcname
    } else {
        dev.device_name.as_str()
    };
    let path = path_list_path(&user.id, pctype, device_name);
    match state.sb.get_fs(&path).await {
        Ok(body) => serde_json::from_str(&body)
            .map_err(|e| AppError::Internal(format!("path-list parse: {}", e))),
        Err(AppError::NotFound(_)) => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Project list for a device. Sourced from the SB-side `path-list.md`
/// registry (maintained by the mini-oc-gui end-side), NOT the device's
/// live API — the portal keeps working while the device is offline.
async fn device_projects(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((pctype, pcname)): Path<(String, String)>,
) -> AppResult<AxumJson<Vec<ProjectInfo>>> {
    let user = current_user(&state, &headers).await?;
    let (_, _) = parse_device_path(&pctype, &pcname)?;
    ensure_device_allowed(&user, &pcname)?;
    let entries = load_path_list(&state, &user, &pctype, &pcname).await?;
    let projects = entries
        .into_iter()
        .map(|e| ProjectInfo {
            path: e.path,
            last_opened_at: e.last_opened_at,
        })
        .collect();
    Ok(AxumJson(projects))
}

#[derive(Deserialize)]
struct SessionsQuery {
    directory: String,
}

/// Session list for a project directory: the `sections[]` ids recorded
/// in `path-list.md` for the matching path entry.
async fn device_sessions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((pctype, pcname)): Path<(String, String)>,
    Query(q): Query<SessionsQuery>,
) -> AppResult<AxumJson<Vec<SessionInfo>>> {
    let user = current_user(&state, &headers).await?;
    let (_, _) = parse_device_path(&pctype, &pcname)?;
    ensure_device_allowed(&user, &pcname)?;
    let entries = load_path_list(&state, &user, &pctype, &pcname).await?;
    let sessions = entries
        .into_iter()
        .find(|e| e.path == q.directory)
        .map(|e| {
            e.sections
                .into_iter()
                .map(|id| SessionInfo {
                    updated_at: e.last_opened_at.clone(),
                    title: None,
                    id,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(AxumJson(sessions))
}

#[derive(Deserialize)]
struct CreateSessionBody {
    directory: String,
    title: Option<String>,
}

async fn device_create_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((pctype, pcname)): Path<(String, String)>,
    AxumJson(body): AxumJson<CreateSessionBody>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let user = current_user(&state, &headers).await?;
    let (_, _) = parse_device_path(&pctype, &pcname)?;
    ensure_device_allowed(&user, &pcname)?;
    let df = state.devices.load().await?;
    let device = find_device(&df, &pctype, &pcname)?;
    let client = cached_device_client(&state, device);
    let created: CreatedSession = client
        .create_session(&body.directory, body.title.as_deref())
        .await?;
    let url = compute_jump_url(&state, device, &body.directory, &created.id).await?;
    let (user, pass) = device_creds_for(&state, device);
    let url = append_auth_token(&url, &user, &pass);
    Ok(AxumJson(serde_json::json!({
        "id": created.id,
        "directory": created.directory,
        "jump_url": url,
    })))
}

/// Resolve the deep-link URL for a session on a given device.
///
/// Base URL priority after the config.md removal: devices.json
/// `public_url`, falling back to `portal_base/{b64pc}` (the unified-domain
/// path-prefix scheme) when the device has no usable URL. On a failed
/// resolve we degrade silently rather than 5xx — the caller has no way
/// to recover.
async fn compute_jump_url(
    state: &AppState,
    device: &Device,
    directory: &str,
    session_id: &str,
) -> AppResult<String> {
    let resolved = resolve_base_url(&JumpInput {
        device_key: device.key(),
        override_url: None,
        default_base_url: None,
        devices_public_url: Some(device.public_url.clone()),
    })
    .unwrap_or_else(|_| {
        // Fallback to the unified-domain path-prefix URL.
        format!(
            "{}/{}",
            state.config.portal_base.trim_end_matches('/'),
            pcname_b64(&device.pcname)
        )
    });

    Ok(build_jump_url(&resolved, directory, session_id))
}

#[derive(Deserialize)]
struct JumpQuery {
    directory: String,
    session: String,
    format: Option<String>,
}

async fn device_jump(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((pctype, pcname)): Path<(String, String)>,
    Query(q): Query<JumpQuery>,
) -> AppResult<impl IntoResponse> {
    let user = current_user(&state, &headers).await?;
    let (_, _) = parse_device_path(&pctype, &pcname)?;
    ensure_device_allowed(&user, &pcname)?;
    let df = state.devices.load().await?;
    let device = df
        .devices
        .iter()
        .find(|d| d.pctype == pctype && d.pcname == pcname)
        .ok_or(AppError::NotFound("device".into()))?;
    let url = compute_jump_url(&state, device, &q.directory, &q.session).await?;
    let (user, pass) = device_creds_for(&state, device);
    let url = append_auth_token(&url, &user, &pass);
    if q.format.as_deref() == Some("json") {
        return Ok(AxumJson(serde_json::json!({"jump_url": url})).into_response());
    }
    Ok(Redirect::to(&url).into_response())
}

// ---------------------------------------------------------------------------
// Admin (super-admin; login = SB_PASSWORD from env)
// ---------------------------------------------------------------------------

/// Constant-time equality over SHA-256 digests (always equal length), so
/// the admin password check never short-circuits on the first differing
/// byte.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut acc = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        acc |= x ^ y;
    }
    acc == 0
}

fn verify_admin_password(entered: &str, expected: &str) -> bool {
    use sha2::{Digest, Sha256};
    let ha = Sha256::digest(entered.as_bytes());
    let hb = Sha256::digest(expected.as_bytes());
    constant_time_eq(&ha, &hb)
}

#[derive(Deserialize)]
struct AdminLoginBody {
    password: String,
}

async fn admin_login(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumJson(body): AxumJson<AdminLoginBody>,
) -> AppResult<impl IntoResponse> {
    let ip = extract_ip(&headers);
    // Separate rate-limit bucket from user key login (same limiter).
    let bucket = format!("admin:{ip}");
    state.rate_limiter.check(&bucket).await?;

    if !verify_admin_password(&body.password, &state.config.sb_password) {
        state.rate_limiter.record_failure(&bucket).await;
        return Err(AppError::Unauthorized("bad admin password".into()));
    }
    state.rate_limiter.reset(&bucket).await;

    let cookie = sign_cookie(ADMIN_SUBJECT, &state.config.cookie_key, ADMIN_COOKIE_TTL).unwrap();
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(
        axum::http::header::SET_COOKIE,
        format!(
            "{}={}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}",
            ADMIN_COOKIE_NAME,
            cookie,
            ADMIN_COOKIE_TTL.as_secs()
        )
        .parse()
        .unwrap(),
    );
    Ok((
        StatusCode::OK,
        resp_headers,
        AxumJson(serde_json::json!({"ok": true})),
    ))
}

async fn admin_logout() -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    let expired = format!(
        "{}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0",
        ADMIN_COOKIE_NAME
    );
    headers.insert(axum::http::header::SET_COOKIE, expired.parse().unwrap());
    (StatusCode::OK, headers, AxumJson(serde_json::json!({"ok": true})))
}

/// Server basics for the admin dashboard: bind/port, identity (hostname,
/// local IPs), SB wiring, uptime and registry sizes.
async fn admin_info(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult<AxumJson<serde_json::Value>> {
    require_admin(&headers, &state.config.cookie_key)?;
    let hostname = hostname::get()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let local_ips: Vec<String> = local_ip_address::list_afinet_netifas()
        .map(|v| v.into_iter().map(|(_, ip)| ip.to_string()).collect())
        .unwrap_or_default();
    let user_count = state.users.load().await?.users.len();
    let device_count = state.devices.load().await?.devices.len();
    Ok(AxumJson(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "web_bind": state.config.web_bind,
        "web_port": state.config.web_port,
        "portal_base": state.config.portal_base,
        "sb_base_url": state.config.sb_base_url,
        "sb_user": state.config.sb_user,
        "sb_password": state.config.sb_password,
        "rathole_key": state.config.rathole_key,
        "hostname": hostname,
        "local_ips": local_ips,
        "uptime_secs": state.started_at.elapsed().as_secs(),
        "user_count": user_count,
        "device_count": device_count,
    })))
}

/// Generate a key guaranteed unique within `uf` (62^32 makes collisions
/// practically impossible; the loop is just belt-and-braces).
fn generate_key_unique(uf: &UsersFile) -> String {
    loop {
        let k = generate_key();
        if !uf.users.iter().any(|u| u.key == k) {
            return k;
        }
    }
}

async fn admin_users_list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult<AxumJson<Vec<PortalUser>>> {
    require_admin(&headers, &state.config.cookie_key)?;
    // Full keys are returned on purpose: the admin is the highest-privilege
    // account and must be able to (re)distribute login keys.
    let uf = state.users.load().await?;
    Ok(AxumJson(uf.users))
}

#[derive(Deserialize)]
struct UserBody {
    name: String,
    /// 设备清单：每项 {d-name: 设备名称, name: 服务名, port: 端口}。
    devices: Vec<UserDevice>,
    /// SB存储配置分配（域名/账号/密码）：可选；None 时 create 落默认值
    /// （域名 https://md.isoops.com、空凭据），update 保留原值。
    #[serde(default)]
    sb: Option<UserSbConfig>,
}

impl UserBody {
    fn validate(&self) -> AppResult<()> {
        validate_user_name(&self.name)?;
        validate_user_devices(&self.devices)?;
        if let Some(sb) = &self.sb {
            validate_sb_config(sb)?;
        }
        Ok(())
    }
}

async fn admin_users_create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumJson(body): AxumJson<UserBody>,
) -> AppResult<AxumJson<PortalUser>> {
    require_admin(&headers, &state.config.cookie_key)?;
    body.validate()?;
    // Pull the registry straight from the remote config document
    // (web/opencode/config.md served by SB): the 6-digit id space is
    // small, so uniqueness must be checked against the freshest
    // remote state. UsersStore::load always hits SB — no cache.
    let mut uf = state.users.load().await?;
    if uf.users.iter().any(|u| u.name == body.name) {
        return Err(AppError::InvalidTarget(format!(
            "user name already exists: {}",
            body.name
        )));
    }
    let now = chrono::Local::now().to_rfc3339();
    let user = PortalUser {
        id: generate_user_id_unique(&uf),
        name: body.name,
        key: generate_key_unique(&uf),
        cloud_ip: crate::users::DEFAULT_CLOUD_IP.to_string(),
        last_used_at: None,
        devices: body.devices,
        sb: body.sb.unwrap_or_default(),
        created_at: Some(now.clone()),
        updated_at: Some(now),
    };
    uf.users.push(user.clone());
    state.users.save(&uf).await?;
    Ok(AxumJson(user))
}

async fn admin_users_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    AxumJson(body): AxumJson<UserBody>,
) -> AppResult<AxumJson<PortalUser>> {
    require_admin(&headers, &state.config.cookie_key)?;
    body.validate()?;
    let mut uf = state.users.load().await?;
    if uf.users.iter().any(|u| u.id != id && u.name == body.name) {
        return Err(AppError::InvalidTarget(format!(
            "user name already exists: {}",
            body.name
        )));
    }
    let user = uf
        .users
        .iter_mut()
        .find(|u| u.id == id)
        .ok_or(AppError::NotFound("user".into()))?;
    user.name = body.name;
    user.devices = body.devices;
    if let Some(sb) = body.sb {
        user.sb = sb;
    }
    user.updated_at = Some(chrono::Local::now().to_rfc3339());
    let updated = user.clone();
    state.users.save(&uf).await?;
    Ok(AxumJson(updated))
}

async fn admin_users_regenerate_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> AppResult<AxumJson<serde_json::Value>> {
    require_admin(&headers, &state.config.cookie_key)?;
    let mut uf = state.users.load().await?;
    let key = generate_key_unique(&uf);
    let user = uf
        .users
        .iter_mut()
        .find(|u| u.id == id)
        .ok_or(AppError::NotFound("user".into()))?;
    user.key = key.clone();
    user.updated_at = Some(chrono::Local::now().to_rfc3339());
    state.users.save(&uf).await?;
    Ok(AxumJson(serde_json::json!({"key": key})))
}

async fn admin_users_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> AppResult<AxumJson<serde_json::Value>> {
    require_admin(&headers, &state.config.cookie_key)?;
    let mut uf = state.users.load().await?;
    let before = uf.users.len();
    uf.users.retain(|u| u.id != id);
    if uf.users.len() == before {
        return Err(AppError::NotFound("user".into()));
    }
    state.users.save(&uf).await?;
    Ok(AxumJson(serde_json::json!({"ok": true})))
}

// SPA fallback: when a request misses a static file, `ServeDir::fallback`
// (`build_router`) serves `index.html` so vue-router history mode works on
// refresh. See `build_router` for the wiring.