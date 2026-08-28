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
        resp.json::<Vec<ProjectInfo>>()
            .await
            .map_err(|e| AppError::Internal(format!("projects parse: {}", e)))
    }

    pub async fn sessions(&self, directory: &str) -> AppResult<Vec<SessionInfo>> {
        let encoded = urlencoding::encode(directory);
        let resp = self
            .send_basic(reqwest::Method::GET, &format!("/session?directory={}", encoded))
            .await?;
        resp.json::<Vec<SessionInfo>>()
            .await
            .map_err(|e| AppError::Internal(format!("sessions parse: {}", e)))
    }

    pub async fn create_session(&self, directory: &str, title: Option<&str>) -> AppResult<CreatedSession> {
        let url = format!("{}/api/session", self.base_url);
        let body = serde_json::json!({
            "directory": directory,
            "title": title,
        });
        let resp = self
            .http
            .post(&url)
            .basic_auth(&self.username, Some(&self.password))
            .json(&body)
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
        resp.json::<CreatedSession>()
            .await
            .map_err(|e| AppError::Internal(format!("create_session parse: {}", e)))
    }
}