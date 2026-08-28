use crate::auth::{sign_cookie, verify_cookie};
use crate::config_md::{validate_pcname, PortalConfig};
use crate::devices::DevicesFile;
use crate::error::{AppError, AppResult};
use crate::jump::{build_jump_url, validate_base_url};
use crate::proxy::{CreatedSession, DeviceClient, ProjectInfo, SessionInfo};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Json as AxumJson, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

const COOKIE_NAME: &str = "mini_oc_web_session";
const COOKIE_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/devices", get(devices))
        .route("/api/config", get(get_config).put(put_config))
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
        .route("/api/manual/:device_id/sessions", post(manual_create_session))
        .fallback(static_handler)
        .with_state(Arc::new(state))
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

fn set_session_cookie(username: &str, key: &[u8]) -> String {
    let cookie = sign_cookie(username, key, COOKIE_TTL).unwrap();
    format!(
        "{}={}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}",
        COOKIE_NAME,
        cookie,
        COOKIE_TTL.as_secs()
    )
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok\n")
}

#[derive(Deserialize)]
struct LoginBody {
    username: String,
    password: String,
}

async fn login(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumJson(body): AxumJson<LoginBody>,
) -> AppResult<impl IntoResponse> {
    let ip = extract_ip(&headers);
    state.rate_limiter.check(&ip).await?;

    if body.username != state.config.username || body.password != state.config.password {
        state.rate_limiter.record_failure(&ip).await;
        return Err(AppError::Unauthorized("bad credentials".into()));
    }
    state.rate_limiter.reset(&ip).await;

    let cookie = set_session_cookie(&body.username, &state.config.cookie_key);
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(axum::http::header::SET_COOKIE, cookie.parse().unwrap());
    Ok((StatusCode::OK, resp_headers, AxumJson(serde_json::json!({"ok": true}))))
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
    public_url: String,
    base_url: String,
    overridden: bool,
    online: bool,
    version: Option<String>,
}

async fn devices(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult<AxumJson<Vec<DeviceView>>> {
    require_session(&headers, &state.config.cookie_key)?;
    let df: DevicesFile = state.devices.load().await?;
    let mut out = Vec::with_capacity(df.devices.len());
    for d in df.devices {
        let client = DeviceClient::new(&d.public_url, "x", "x");
        let online = client.health().await.unwrap_or(false);
        out.push(DeviceView {
            base_url: d.public_url.clone(),
            overridden: false,
            pctype: d.pctype.clone(),
            pcname: d.pcname.clone(),
            public_url: d.public_url,
            online,
            version: d.version,
        });
    }
    Ok(AxumJson(out))
}

#[derive(Deserialize)]
struct PcQuery {
    pcname: Option<String>,
}

async fn get_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<PcQuery>,
) -> AppResult<AxumJson<PortalConfig>> {
    require_session(&headers, &state.config.cookie_key)?;
    let pcname = q.pcname.ok_or(AppError::InvalidPcname("missing pcname".into()))?;
    validate_pcname(&pcname)?;
    let path = format!("serv/web/{}/config.md", pcname);
    let body = match state.sb.get_fs(&path).await {
        Ok(b) => b,
        Err(AppError::NotFound(_)) => {
            return Ok(AxumJson(crate::config_md::default_config(&pcname)));
        }
        Err(e) => return Err(e),
    };
    let cfg: PortalConfig = serde_json::from_str(&body)
        .map_err(|e| AppError::Internal(format!("config parse: {}", e)))?;
    Ok(AxumJson(cfg))
}

async fn put_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<PcQuery>,
    AxumJson(mut cfg): AxumJson<PortalConfig>,
) -> AppResult<AxumJson<PortalConfig>> {
    require_session(&headers, &state.config.cookie_key)?;
    let pcname = q.pcname.ok_or(AppError::InvalidPcname("missing pcname".into()))?;
    validate_pcname(&pcname)?;
    let path = format!("serv/web/{}/config.md", pcname);
    if let Ok(existing) = state.sb.get_fs(&path).await {
        if let Ok(existing_cfg) = serde_json::from_str::<PortalConfig>(&existing) {
            cfg.version = existing_cfg.version;
        }
    }
    cfg.pcname = pcname.clone();
    cfg.updated_at = chrono::Local::now().to_rfc3339();
    let body = serde_json::to_string_pretty(&cfg)
        .map_err(|e| AppError::Internal(format!("config serialize: {}", e)))?;
    state.sb.put_fs(&path, &body).await?;
    Ok(AxumJson(cfg))
}

fn parse_device_path(pctype: &str, pcname: &str) -> AppResult<(String, String)> {
    if pctype != "macos" && pctype != "windows" {
        return Err(AppError::InvalidTarget(format!("bad pctype: {}", pctype)));
    }
    validate_pcname(pcname)?;
    Ok((pctype.to_string(), pcname.to_string()))
}

async fn device_projects(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((pctype, pcname)): Path<(String, String)>,
) -> AppResult<AxumJson<Vec<ProjectInfo>>> {
    require_session(&headers, &state.config.cookie_key)?;
    let (_, _) = parse_device_path(&pctype, &pcname)?;
    let df = state.devices.load().await?;
    let device = df
        .devices
        .iter()
        .find(|d| d.pctype == pctype && d.pcname == pcname)
        .ok_or(AppError::NotFound("device".into()))?;
    let client = DeviceClient::new(
        &device.public_url,
        &state.config.username,
        &state.config.password,
    );
    let projects = client.projects().await?;
    Ok(AxumJson(projects))
}

#[derive(Deserialize)]
struct SessionsQuery {
    directory: String,
}

async fn device_sessions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((pctype, pcname)): Path<(String, String)>,
    Query(q): Query<SessionsQuery>,
) -> AppResult<AxumJson<Vec<SessionInfo>>> {
    require_session(&headers, &state.config.cookie_key)?;
    let (_, _) = parse_device_path(&pctype, &pcname)?;
    let df = state.devices.load().await?;
    let device = df
        .devices
        .iter()
        .find(|d| d.pctype == pctype && d.pcname == pcname)
        .ok_or(AppError::NotFound("device".into()))?;
    let client = DeviceClient::new(
        &device.public_url,
        &state.config.username,
        &state.config.password,
    );
    let sessions = client.sessions(&q.directory).await?;
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
    require_session(&headers, &state.config.cookie_key)?;
    let (_, _) = parse_device_path(&pctype, &pcname)?;
    let df = state.devices.load().await?;
    let device = df
        .devices
        .iter()
        .find(|d| d.pctype == pctype && d.pcname == pcname)
        .ok_or(AppError::NotFound("device".into()))?;
    let client = DeviceClient::new(
        &device.public_url,
        &state.config.username,
        &state.config.password,
    );
    let created: CreatedSession = client.create_session(&body.directory, body.title.as_deref()).await?;
    let url = compute_jump_url(&state, &device.public_url, &body.directory, &created.id).await?;
    Ok(AxumJson(serde_json::json!({
        "id": created.id,
        "directory": created.directory,
        "jump_url": url,
    })))
}

async fn manual_create_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(device_id): Path<String>,
    AxumJson(_body): AxumJson<CreateSessionBody>,
) -> Result<axum::response::Response, AppError> {
    require_session(&headers, &state.config.cookie_key)?;
    validate_pcname(&device_id)?;
    let _ = (state, device_id);
    Err(AppError::Internal("manual devices not yet wired".into()))
}

async fn compute_jump_url(
    state: &AppState,
    public_url: &str,
    directory: &str,
    session_id: &str,
) -> AppResult<String> {
    let _ = state;
    validate_base_url(public_url)?;
    Ok(build_jump_url(public_url, directory, session_id))
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
    require_session(&headers, &state.config.cookie_key)?;
    let (_, _) = parse_device_path(&pctype, &pcname)?;
    let df = state.devices.load().await?;
    let device = df
        .devices
        .iter()
        .find(|d| d.pctype == pctype && d.pcname == pcname)
        .ok_or(AppError::NotFound("device".into()))?;
    let url = compute_jump_url(&state, &device.public_url, &q.directory, &q.session).await?;
    if q.format.as_deref() == Some("json") {
        return Ok(AxumJson(serde_json::json!({"jump_url": url})).into_response());
    }
    Ok(Redirect::to(&url).into_response())
}

async fn static_handler(_uri: axum::http::Uri) -> impl IntoResponse {
    let body = r#"<!doctype html><html><body><h1>mini-oc-web</h1><p>SPA placeholder. Build web/dist for full UI.</p></body></html>"#;
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
}
