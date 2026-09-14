//! SilverBullet client.
//!
//! Authentication mirrors `mini-oc-gui`'s `RemoteClient`:
//! - POST `/.auth` with form `username` + `password`
//! - Cookie name is derived from the host: `https://md.isoops.com` →
//!   `auth_md_isoops_com` (dots → underscores).
//! - On a 401 from `/.fs/*`, we transparently re-login once and retry.
//!
//! This is the only path that actually works against a real SB 2.9+; the
//! earlier "Basic POST /" assumed a non-existent API and returned 401 on every
//! `/.fs/` call (which is what was blocking the BFF in local dev).

use crate::error::{AppError, AppResult};
use reqwest::header::{ACCEPT, CONTENT_TYPE, COOKIE, HeaderMap, HeaderValue, SET_COOKIE};
use reqwest::{Client, StatusCode};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug)]
pub struct SbClient {
    base_url: String,
    cookie_name: String,
    username: String,
    password: String,
    cookie: Mutex<Option<String>>,
    http: Client,
}

impl SbClient {
    pub fn new(
        base_url: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> AppResult<Self> {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        let cookie_name = derive_cookie_name(&base_url);
        let http = Client::builder()
            .cookie_store(false)
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| AppError::Internal(format!("reqwest build: {}", e)))?;
        Ok(Self {
            base_url,
            cookie_name,
            username: username.into(),
            password: password.into(),
            cookie: Mutex::new(None),
            http,
        })
    }

    /// The cookie name the server uses for this host (e.g. `auth_md_isoops_com`).
    #[must_use]
    pub fn cookie_name(&self) -> &str {
        &self.cookie_name
    }

    pub fn cookie(&self) -> Option<String> {
        self.cookie.lock().unwrap().clone()
    }

    /// `POST /.auth` form login; extracts the named `set-cookie` into memory.
    /// Idempotent: clears any prior cookie first.
    pub async fn login(&self) -> AppResult<()> {
        *self.cookie.lock().unwrap() = None;
        let login_url = format!("{}/.auth", self.base_url);
        let resp = self
            .http
            .post(&login_url)
            .header(ACCEPT, "*/*")
            .form(&[("username", self.username.as_str()), ("password", self.password.as_str())])
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("sb login: {}", e)))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(AppError::Internal(format!(
                "sb login status: {} (check SB_USER/SB_PASSWORD)",
                status
            )));
        }
        let cookie = extract_cookie(resp.headers(), &self.cookie_name).ok_or_else(|| {
            AppError::Internal(format!(
                "sb login response missing cookie '{}'",
                self.cookie_name
            ))
        })?;
        *self.cookie.lock().unwrap() = Some(cookie);
        Ok(())
    }

    async fn re_login(&self) -> AppResult<()> {
        self.login().await
    }

    /// `GET /.fs/<path>` with auto-relogin on 401.
    pub async fn get_fs(&self, path: &str) -> AppResult<String> {
        let url = format!("{}/.fs/{}", self.base_url, path.trim_start_matches('/'));
        let first = self.send_get(&url).await?;
        if first.status() == StatusCode::UNAUTHORIZED {
            self.re_login().await?;
            let second = self.send_get(&url).await?;
            return Self::read_text(second, path).await;
        }
        Self::read_text(first, path).await
    }

    /// `PUT /.fs/<path>` with auto-relogin on 401.
    pub async fn put_fs(&self, path: &str, body: &str) -> AppResult<()> {
        let url = format!("{}/.fs/{}", self.base_url, path.trim_start_matches('/'));
        let first = self.send_put(&url, body).await?;
        if first.status() == StatusCode::UNAUTHORIZED {
            self.re_login().await?;
            let second = self.send_put(&url, body).await?;
            return Self::expect_ok(second, "sb put_fs");
        }
        Self::expect_ok(first, "sb put_fs")
    }

    async fn send_get(&self, url: &str) -> AppResult<reqwest::Response> {
        let mut req = self.http.get(url).header(ACCEPT, "*/*");
        if let Some(c) = self.cookie() {
            if let Ok(v) = HeaderValue::from_str(&c) {
                req = req.header(COOKIE, v);
            }
        }
        req.send()
            .await
            .map_err(|e| AppError::Internal(format!("sb get_fs: {}", e)))
    }

    async fn send_put(&self, url: &str, body: &str) -> AppResult<reqwest::Response> {
        let mut req = self
            .http
            .put(url)
            .header(ACCEPT, "*/*")
            .header(CONTENT_TYPE, "text/markdown; charset=utf-8")
            .body(body.to_string());
        if let Some(c) = self.cookie() {
            if let Ok(v) = HeaderValue::from_str(&c) {
                req = req.header(COOKIE, v);
            }
        }
        req.send()
            .await
            .map_err(|e| AppError::Internal(format!("sb put_fs: {}", e)))
    }

    async fn read_text(resp: reqwest::Response, path: &str) -> AppResult<String> {
        let status = resp.status();
        if status == StatusCode::NOT_FOUND {
            return Err(AppError::NotFound(path.to_string()));
        }
        if !status.is_success() {
            return Err(AppError::Internal(format!(
                "sb get_fs status: {}",
                status
            )));
        }
        resp.text()
            .await
            .map_err(|e| AppError::Internal(format!("sb get_fs body: {}", e)))
    }

    fn expect_ok(resp: reqwest::Response, ctx: &str) -> AppResult<()> {
        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else {
            Err(AppError::Internal(format!(
                "{} status: {}",
                ctx, status
            )))
        }
    }
}

/// `https://md.isoops.com` → `auth_md_isoops_com`.
fn derive_cookie_name(base_url: &str) -> String {
    let after_scheme = base_url.split_once("://").map_or(base_url, |(_, r)| r);
    let host_and_port = after_scheme.split('/').next().unwrap_or(after_scheme);
    let host = host_and_port.split(':').next().unwrap_or(host_and_port);
    format!("auth_{}", host.replace('.', "_"))
}

fn extract_cookie(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    let prefix = format!("{cookie_name}=");
    for value in headers.get_all(SET_COOKIE) {
        if let Ok(s) = value.to_str() {
            if let Some(rest) = s.strip_prefix(&prefix) {
                if let Some(cookie_part) = rest.split(';').next() {
                    return Some(format!("{prefix}{cookie_part}"));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_cookie_name_basic() {
        assert_eq!(
            derive_cookie_name("https://md.isoops.com"),
            "auth_md_isoops_com"
        );
        assert_eq!(
            derive_cookie_name("http://127.0.0.1:3000"),
            "auth_127_0_0_1"
        );
        assert_eq!(derive_cookie_name("http://silverbullet:3000"), "auth_silverbullet");
    }
}