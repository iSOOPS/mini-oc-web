use crate::error::{AppError, AppResult};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub path: String,
    #[serde(rename = "lastOpenedAt", default)]
    pub last_opened_at: Option<String>,
}

/// Wire format of `oc serve`'s `GET /project`: entries look like
/// `{"id":"global","worktree":"/","time":{"created":1784163362673,
/// "updated":1789219158527},"sandboxes":[]}`. The BFF re-projects this
/// onto [`ProjectInfo`] (path = worktree, lastOpenedAt = time.updated)
/// so the SPA keeps a stable shape.
#[derive(Debug, Deserialize)]
struct RawProject {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    worktree: Option<String>,
    /// Legacy/alternate shape kept for compatibility with older mocks.
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    time: Option<RawTime>,
    #[serde(default, rename = "lastOpenedAt")]
    last_opened_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawTime {
    #[serde(default)]
    updated: Option<i64>,
}

fn ms_to_rfc3339(ms: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.to_rfc3339())
}

impl From<RawProject> for ProjectInfo {
    fn from(r: RawProject) -> Self {
        let path = r
            .worktree
            .or(r.path)
            .or(r.id)
            .unwrap_or_default();
        let last_opened_at = r
            .last_opened_at
            .or_else(|| r.time.and_then(|t| t.updated).and_then(ms_to_rfc3339));
        ProjectInfo {
            path,
            last_opened_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatedSession {
    pub id: String,
    #[serde(default)]
    pub directory: Option<String>,
}

/// Wire shapes for `POST /api/session`: real opencode answers
/// `{"data":{id,title,location:{directory}}}`; bare `{id,...}` is kept
/// for mock compatibility.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CreatedWire {
    Wrapped { data: RawCreatedData },
    Bare(RawCreatedData),
}

#[derive(Debug, Deserialize)]
struct RawCreatedData {
    id: String,
    #[serde(default)]
    location: Option<RawLocation>,
    #[serde(default)]
    directory: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawLocation {
    #[serde(default)]
    directory: Option<String>,
}

impl From<RawCreatedData> for CreatedSession {
    fn from(r: RawCreatedData) -> Self {
        CreatedSession {
            id: r.id,
            directory: r
                .location
                .and_then(|l| l.directory)
                .or(r.directory),
        }
    }
}

#[derive(Clone)]
pub struct DeviceClient {
    base_url: String,
    username: String,
    password: String,
    http: Client,
}

impl DeviceClient {
    pub fn new(base_url: impl Into<String>, username: impl Into<String>, password: impl Into<String>) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest build");
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            username: username.into(),
            password: password.into(),
            http,
        }
    }

    pub async fn health(&self) -> AppResult<bool> {
        let url = format!("{}/health", self.base_url);
        match self.http.get(&url).send().await {
            Ok(r) => Ok(r.status().is_success()),
            Err(_) => Ok(false),
        }
    }

    async fn send_basic(&self, method: reqwest::Method, path: &str) -> AppResult<reqwest::Response> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .http
            .request(method, &url)
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() || e.is_timeout() || e.is_request() {
                    AppError::DeviceOffline(format!("{} {}: {}", self.base_url, path, e))
                } else {
                    AppError::Internal(format!("device {} {}: {}", self.base_url, path, e))
                }
            })?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(AppError::DeviceAuthFailed(format!(
                "{} returned {}",
                self.base_url, status
            )));
        }
        if !status.is_success() {
            return Err(AppError::Internal(format!(
                "{} returned {}",
                self.base_url, status
            )));
        }
        Ok(resp)
    }

    pub async fn projects(&self) -> AppResult<Vec<ProjectInfo>> {
        let resp = self.send_basic(reqwest::Method::GET, "/project").await?;
        let raw: Vec<RawProject> = resp
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("projects parse: {}", e)))?;
        Ok(raw.into_iter().map(ProjectInfo::from).collect())
    }

    pub async fn create_session(&self, directory: &str, title: Option<&str>) -> AppResult<CreatedSession> {
        let url = format!("{}/api/session", self.base_url);
        // Real opencode contract: directory rides inside `location`; the
        // old `{directory}` top-level field was silently ignored by the
        // server (falling back to its own default dir). Omit `title`
        // entirely when not given so the server generates one.
        let mut body = serde_json::Map::new();
        body.insert(
            "location".to_string(),
            serde_json::json!({ "directory": directory }),
        );
        if let Some(t) = title {
            body.insert("title".to_string(), serde_json::json!(t));
        }
        let resp = self
            .http
            .post(&url)
            .basic_auth(&self.username, Some(&self.password))
            .json(&serde_json::Value::Object(body))
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("create_session: {}", e)))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(AppError::DeviceAuthFailed(format!(
                "{} returned {}",
                self.base_url, status
            )));
        }
        if !status.is_success() {
            return Err(AppError::Internal(format!(
                "create_session {}: {}",
                self.base_url, status
            )));
        }
        let wire: CreatedWire = resp
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("create_session parse: {}", e)))?;
        Ok(match wire {
            CreatedWire::Wrapped { data } => CreatedSession::from(data),
            CreatedWire::Bare(d) => CreatedSession::from(d),
        })
    }
}