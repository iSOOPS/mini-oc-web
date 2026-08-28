use crate::error::{AppError, AppResult};
use reqwest::Client;
use std::sync::Mutex;

#[derive(Debug)]
pub struct SbClient {
    base_url: String,
    username: String,
    password: String,
    cookie: Mutex<Option<String>>,
    http: Client,
}

impl SbClient {
    pub fn new(base_url: impl Into<String>, username: impl Into<String>, password: impl Into<String>) -> AppResult<Self> {
        let http = Client::builder()
            .cookie_store(false)
            .build()
            .map_err(|e| AppError::Internal(format!("reqwest build: {}", e)))?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            username: username.into(),
            password: password.into(),
            cookie: Mutex::new(None),
            http,
        })
    }

    pub fn cookie(&self) -> Option<String> {
        self.cookie.lock().unwrap().clone()
    }

    pub async fn login(&self) -> AppResult<()> {
        let resp = self
            .http
            .post(format!("{}/", self.base_url))
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("sb login: {}", e)))?;
        if !resp.status().is_success() {
            return Err(AppError::Internal(format!("sb login status: {}", resp.status())));
        }
        let cookie = resp
            .headers()
            .get("set-cookie")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(';').next().unwrap_or(s).to_string());
        *self.cookie.lock().unwrap() = cookie;
        Ok(())
    }

    pub async fn get_fs(&self, path: &str) -> AppResult<String> {
        let url = format!("{}/.fs/{}", self.base_url, path.trim_start_matches('/'));
        let mut req = self.http.get(&url);
        if let Some(c) = self.cookie() {
            req = req.header("cookie", c);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("sb get_fs: {}", e)))?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(AppError::NotFound(path.to_string()));
        }
        if !status.is_success() {
            return Err(AppError::Internal(format!("sb get_fs status: {}", status)));
        }
        resp.text().await.map_err(|e| AppError::Internal(format!("sb get_fs body: {}", e)))
    }

    pub async fn put_fs(&self, path: &str, body: &str) -> AppResult<()> {
        let url = format!("{}/.fs/{}", self.base_url, path.trim_start_matches('/'));
        let mut req = self.http.put(&url).body(body.to_string());
        if let Some(c) = self.cookie() {
            req = req.header("cookie", c);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("sb put_fs: {}", e)))?;
        if !resp.status().is_success() {
            return Err(AppError::Internal(format!("sb put_fs status: {}", resp.status())));
        }
        Ok(())
    }
}
