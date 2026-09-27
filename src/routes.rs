use crate::auth::{sign_cookie, verify_cookie};
use crate::error::{AppError, AppResult};
use crate::jump::{
    append_auth_token, build_jump_url, pcname_b64,
};
use crate::probe::{probe_device_status, verify_device_identity, DeviceProbeResult};
use crate::proxy::{CreatedSession, DeviceClient, ProjectInfo, SessionInfo};
use crate::state::AppState;
use crate::users::{
    generate_key, generate_user_id_unique, validate_sb_config, validate_user_devices,
    validate_user_name, PortalUser, UserDevice, UserSbConfig, UsersFile,
};
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json as AxumJson, Router};
use futures::future::join_all;
use regex::Regex;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// SB path of a user's per-device project/session registry, written by
/// the mini-oc-gui end-side. Multi-tenant layout: `user_id` is the
/// portal user's id (NOT the admin SB_USER); `pctype` comes from the
/// device entry's `UserDevice.pctype`; the third segment is the device
/// SERVICE NAME (`UserDevice.name`, same as the URL `pcname`) — verified
/// against the live registry (`.../667004/macos/service-mac/path-list.md`),
/// NOT the client-reported `device-name`.
const PATH_LIST_PATH: &str = "serv/opencode/{user_id}/{pctype}/{pcname}/path-list.md";

fn path_list_path(user_id: &str, pctype: &str, pcname: &str) -> String {
    PATH_LIST_PATH
        .replace("{user_id}", user_id)
        .replace("{pctype}", pctype)
        .replace("{pcname}", pcname)
}

/// One session section of `path-list.md` — live wire shape (verified
/// against the SB document): `{"id":"ses_…","title":"…","directory":"…",
/// "createdAt":"…","updatedAt":"…"}` (camelCase timestamps; `directory`
/// is tolerated and ignored — the entry's `path` already identifies the
/// project).
#[derive(Debug, Clone, Deserialize)]
struct PathSection {
    id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default, rename = "updatedAt")]
    updated_at: Option<String>,
}

/// One entry of `path-list.md`. Wire shape mirrors the mini-oc-gui
/// end-side (camelCase timestamps, verified against the live SB
/// document): `{"path":"D:/x","sections":[{…PathSection…}],"createdAt":
/// "2026-09-09T23:25:10+08:00","lastOpenedAt":"2026-09-13T00:59:41+08:00"}`.
/// Timestamps are passed through as RFC-3339 strings.
#[derive(Debug, Clone, Deserialize)]
struct PathListEntry {
    path: String,
    #[serde(default)]
    sections: Vec<PathSection>,
    /// Parsed for schema completeness with the mini-oc-gui end-side
    /// wire format, but not consumed by the BFF (project ordering comes
    /// from `lastOpenedAt` only). Cheap to keep — deserializer tolerates
    /// missing field.
    #[serde(default)]
    #[allow(dead_code)]
    created_at: Option<String>,
    #[serde(default, rename = "lastOpenedAt")]
    last_opened_at: Option<String>,
}

/// Whitelist: `[A-Za-z0-9_-]{1,64}` (design doc §7.2). Validates device
/// `pcname` segments in the URL path of every `/api/devices/:pctype/:pcname/...`
/// route, where the pcname segment must be safe to splice into SB paths.
fn validate_pcname(s: &str) -> AppResult<()> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9_-]{1,64}$").unwrap());
    if re.is_match(s) {
        Ok(())
    } else {
        Err(AppError::InvalidPcname(s.to_string()))
    }
}

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
        .route("/api/me/sb", post(me_sb))
        .route("/api/user/info", post(user_info_by_key))
        .route("/api/device-bind", post(device_bind))
        .route("/api/device-status/:port", get(device_status))
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

/// Device-hop Basic-Auth credentials: the signed-in portal user's own
/// account id (username) and login key (password).
///
/// The device-side opencode serve runs with
/// `OPENCODE_SERVER_USERNAME=<user id>` / `OPENCODE_SERVER_PASSWORD=<user
/// key>` (mini-oc-gui configures both from the binding profile it fetches
/// via `POST /api/user/info`), so the BFF's server-side device calls and
/// the browser-side `?auth_token=` deep link authenticate with the same
/// self-derived pair — no per-device credential dialog or portal-wide
/// fallback env pair anymore.
fn device_credentials(user: &PortalUser) -> (String, String) {
    (user.id.clone(), user.key.clone())
}

/// Base URL of the device's local TUI service (mini-oc-gui): the tunnel
/// address plus the 服务端口号 — `/status`, `/project`, `/api/session`
/// all live here. Callers must ensure `public_url` is non-empty first.
fn tui_base_url(ud: &UserDevice) -> String {
    format!("{}:{}", ud.public_url.trim_end_matches('/'), ud.port)
}

/// Base URL of the device's opencode server (web UI + native API): the
/// tunnel address plus the OpenCode 端口号. Browser deep links are built
/// on this. Callers must ensure `public_url` is non-empty first.
fn oc_base_url(ud: &UserDevice) -> String {
    format!("{}:{}", ud.public_url.trim_end_matches('/'), ud.oc_port)
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
/// every assigned device's runtime status probed concurrently.
///
/// Each device is probed at the TUI-service address composed from its
/// admin-assigned tunnel address and 服务端口号: `GET
/// {public_url}:{port}/status` (see [`tui_base_url`]; the same URL base
/// serves `/project` and `/api/session`). Browser deep links use the
/// OpenCode 端口号 instead: `{public_url}:{oc_port}/...`.
///
/// Each device entry carries three independent runtime signals so the SPA
/// can render them separately:
/// - `online` (bool): whether `GET /status` answered at all (regardless of
///   body content). `false` when the probe connect/timeout/non-2xx or the
///   body couldn't be parsed — see `reason`. Also `false` when the answer's
///   platform (`system.os`) disagrees with the entry's `pctype`: several
///   entries can resolve to the same probe URL, and a /status answer
///   from a different device must not paint this entry green — see
///   [`crate::probe::verify_device_identity`].
/// - `opencode_online` (bool): whether `/status`'s `opencode_serve` field
///   is the literal `"running"`. Always `false` when `online` is `false`
///   (no status body to inspect).
/// - `available` (number): roll-up of (bound, online, opencode_online) for
///   color-coding the dot:
///   * `1` = fully usable (bound AND online AND opencode_online)
///   * `0` = reachable but not yet usable (online=true but bound or
///          opencode_online missing)
///   * `-1` = unreachable (online=false — `/status` probe failed)
///
/// Two distinct addresses must not be conflated:
///   * `cloud_ip` (user settings "云服务 IP") — device→cloud direction:
///     the address the device-side TUI uses to call back this BFF
///     (`/api/device-bind`, `/api/user/info`). Never a probe target.
///   * `UserDevice.public_url` (admin-assigned device URL) — cloud→device
///     direction: the ONLY address the BFF probes `<url>/status` with.
/// Entries with an empty `public_url` (legacy data; new writes are
/// validated non-empty) are reported offline with an explicit
/// "not configured" reason instead of borrowing `cloud_ip`.
async fn me(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult<AxumJson<serde_json::Value>> {
    let user = current_user(&state, &headers).await?;
    let probes = user
        .devices
        .iter()
        .map(|d| async move {
            if d.public_url.trim().is_empty() {
                DeviceProbeResult::offline(
                    "device URL not configured (empty public-url) — \
                     ask the admin to fill the device URL in the user's device list",
                )
            } else {
                probe_device_status(tui_base_url(d)).await
            }
        })
        .collect::<Vec<_>>();
    let results = join_all(probes).await;

    let mut enriched: Vec<serde_json::Value> = Vec::with_capacity(user.devices.len());
    for (ud, pr) in user.devices.iter().zip(results.into_iter()) {
        // Reject /status answers that come from a different device
        // (entries sharing one probe URL) — see verify_device_identity.
        let pr = verify_device_identity(&ud.pctype, pr);
        let opencode_online = pr.online
            && pr
                .status
                .as_ref()
                .and_then(|s| s.get("opencode_serve"))
                .and_then(|v| v.as_str())
                == Some("running");

        let available: i8 = if !pr.online {
            -1
        } else if ud.bound && opencode_online {
            1
        } else {
            0
        };

        let mut entry = serde_json::json!({
            "desc": ud.desc,
            "name": ud.name,
            "port": ud.port,
            "oc-port": ud.oc_port,
            "device-name": ud.device_name,
            "pctype": ud.pctype,
            "bound": ud.bound,
            "public-url": ud.public_url,
            "online": pr.online,
            "opencode_online": opencode_online,
            "available": available,
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

/// Probe a device's real state through its own `public_url` (admin-
/// assigned in `UserDevice.public_url` — cloud→device direction; see
/// [`me`] for the address-semantics rationale). An empty `public_url`
/// yields an offline result with a "not configured" reason — the
/// `cloud_ip` user setting (device→cloud callback address) is never a
/// probe target.
async fn device_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(port): Path<u16>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let user = current_user(&state, &headers).await?;
    let ud = user
        .devices
        .iter()
        .find(|d| d.port == port)
        .ok_or_else(|| AppError::Forbidden(format!("port {port} is not in your device list")))?;
    let pr = if ud.public_url.trim().is_empty() {
        DeviceProbeResult::offline(
            "device URL not configured (empty public-url) — \
             ask the admin to fill the device URL in the user's device list",
        )
    } else {
        verify_device_identity(&ud.pctype, probe_device_status(tui_base_url(ud)).await)
    };
    let mut body = serde_json::json!({"online": pr.online});
    if let Some(r) = pr.reason {
        body["reason"] = serde_json::Value::String(r);
    }
    if let Some(s) = pr.status {
        body["status"] = s;
    }
    Ok(AxumJson(body))
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

/// Device connection details for the SPA "详情" dialog: identity, base
/// URL, and the credentials the BFF uses for the device hop — the signed-in
/// user's own account id + login key (see [`device_credentials`]). The
/// password is returned in clear text — the SPA masks it client-side
/// (`*` per char) until the user reveals it. Session-protected like
/// every other /api route.
async fn device_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((pctype, pcname)): Path<(String, String)>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let user = current_user(&state, &headers).await?;
    if pctype != "macos" && pctype != "windows" {
        return Err(AppError::InvalidTarget(format!("bad pctype: {}", pctype)));
    }
    validate_pcname(&pcname)?;
    ensure_device_allowed(&user, &pcname)?;
    let ud = user
        .devices
        .iter()
        .find(|d| d.name == pcname)
        .ok_or_else(|| AppError::NotFound("device".into()))?;
    let (username, password) = device_credentials(&user);
    Ok(AxumJson(serde_json::json!({
        "pctype": ud.pctype,
        "pcname": ud.name,
        "public_url": ud.public_url,
        "port": ud.port,
        "oc_port": ud.oc_port,
        "username": username,
        "password": password,
    })))
}

/// Load the device's `path-list.md` from SilverBullet. The document is
/// located per the multi-tenant layout
/// `serv/opencode/{user_id}/{pctype}/{pcname}/path-list.md`: the user id
/// is the caller's own id and the platform type comes from the caller's
/// device-list entry (设备清单, admin-assigned; falls back to the URL's
/// pctype for legacy entries), while the last segment is the device
/// SERVICE NAME (`UserDevice.name` / URL pcname) — the mini-oc-gui
/// end-side keys its registry by service name, NOT the client-reported
/// `device-name`. A missing document is an empty registry (the end-side
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
    let path = path_list_path(&user.id, pctype, pcname);
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
    if pctype != "macos" && pctype != "windows" {
        return Err(AppError::InvalidTarget(format!("bad pctype: {}", pctype)));
    }
    validate_pcname(&pcname)?;
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
    if pctype != "macos" && pctype != "windows" {
        return Err(AppError::InvalidTarget(format!("bad pctype: {}", pctype)));
    }
    validate_pcname(&pcname)?;
    ensure_device_allowed(&user, &pcname)?;
    let entries = load_path_list(&state, &user, &pctype, &pcname).await?;
    let sessions = entries
        .into_iter()
        .find(|e| e.path == q.directory)
        .map(|e| {
            e.sections
                .into_iter()
                .map(|s| SessionInfo {
                    updated_at: s.updated_at.or_else(|| e.last_opened_at.clone()),
                    title: s.title,
                    id: s.id,
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
    if pctype != "macos" && pctype != "windows" {
        return Err(AppError::InvalidTarget(format!("bad pctype: {}", pctype)));
    }
    validate_pcname(&pcname)?;
    let ud = user
        .devices
        .iter()
        .find(|d| d.name == pcname)
        .ok_or_else(|| AppError::NotFound("device".into()))?;
    let (user_cred, pass_cred) = device_credentials(&user);
    let client = DeviceClient::new(&tui_base_url(ud), &user_cred, &pass_cred);
    let created: CreatedSession = client
        .create_session(&body.directory, body.title.as_deref())
        .await?;
    let url = build_jump_url_for(&state, ud, &body.directory, &created.id);
    let url = append_auth_token(&url, &user_cred, &pass_cred);
    Ok(AxumJson(serde_json::json!({
        "id": created.id,
        "directory": created.directory,
        "jump_url": url,
    })))
}

/// Resolve the deep-link URL for a session on a given device.
///
/// Base URL priority: `UserDevice.public_url` (admin-assigned device
/// base), falling back to `portal_base/{b64pc}` (the unified-domain
/// path-prefix scheme) when the device has no usable URL. On a failed
/// resolve we degrade silently rather than 5xx — the caller has no way
/// to recover.
fn build_jump_url_for(
    state: &AppState,
    ud: &UserDevice,
    directory: &str,
    session_id: &str,
) -> String {
    let resolved = if !ud.public_url.trim().is_empty() {
        oc_base_url(ud)
    } else {
        format!(
            "{}/{}",
            state.config.portal_base.trim_end_matches('/'),
            pcname_b64(&ud.name)
        )
    };
    build_jump_url(&resolved, directory, session_id)
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
    if pctype != "macos" && pctype != "windows" {
        return Err(AppError::InvalidTarget(format!("bad pctype: {}", pctype)));
    }
    validate_pcname(&pcname)?;
    ensure_device_allowed(&user, &pcname)?;
    let ud = user
        .devices
        .iter()
        .find(|d| d.name == pcname)
        .ok_or_else(|| AppError::NotFound("device".into()))?;
    let (user_cred, pass_cred) = device_credentials(&user);
    let url = build_jump_url_for(&state, ud, &q.directory, &q.session);
    let url = append_auth_token(&url, &user_cred, &pass_cred);
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
    Ok(AxumJson(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "web_bind": state.config.web_bind,
        "web_port": state.config.web_port,
        "portal_base": state.config.portal_base,
        "sb_base_url": state.config.sb_base_url,
        "sb_user": state.config.sb_user,
        "sb_password": state.config.sb_password,
        "hostname": hostname,
        "local_ips": local_ips,
        "uptime_secs": state.started_at.elapsed().as_secs(),
        "user_count": user_count,
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
    /// （域名 https://sb.example.com、空凭据），update 保留原值。
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