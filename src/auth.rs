use crate::error::{AppError, AppResult};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

type HmacSha256 = Hmac<Sha256>;

/// HMAC-SHA256 signed cookie value: `base64url(username).base64url(signature)`
pub fn sign_cookie(username: &str, key: &[u8], ttl: Duration) -> AppResult<String> {
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|e| AppError::Internal(format!("hmac key error: {}", e)))?;
    let payload = format!("{}|{}", username, ttl.as_secs());
    mac.update(payload.as_bytes());
    let sig = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.as_bytes());
    Ok(format!("{}.{}", payload_b64, sig))
}

pub fn verify_cookie(cookie: &str, key: &[u8]) -> AppResult<String> {
    let parts: Vec<&str> = cookie.split('.').collect();
    if parts.len() != 2 {
        return Err(AppError::Unauthorized("malformed cookie".into()));
    }
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(parts[0])
        .map_err(|_| AppError::Unauthorized("bad payload encoding".into()))?;
    let payload = String::from_utf8(payload_bytes)
        .map_err(|_| AppError::Unauthorized("bad payload utf8".into()))?;
    let inner: Vec<&str> = payload.split('|').collect();
    if inner.len() != 2 {
        return Err(AppError::Unauthorized("bad payload format".into()));
    }
    let username = inner[0];
    let ttl_secs: u64 = inner[1]
        .parse()
        .map_err(|_| AppError::Unauthorized("bad ttl".into()))?;
    let sig = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|_| AppError::Unauthorized("bad sig encoding".into()))?;
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|e| AppError::Internal(format!("hmac key error: {}", e)))?;
    mac.update(payload.as_bytes());
    mac.verify_slice(&sig)
        .map_err(|_| AppError::Unauthorized("bad signature".into()))?;
    if ttl_secs == 0 {
        return Err(AppError::Unauthorized("expired".into()));
    }
    Ok(username.to_string())
}

/// In-memory per-IP login rate limiter.
/// `limit` failures within `window` blocks the IP.
pub struct RateLimiter {
    limit: u32,
    window: Duration,
    state: Mutex<HashMap<String, Vec<Instant>>>,
}

impl RateLimiter {
    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            limit,
            window,
            state: Mutex::new(HashMap::new()),
        }
    }

    pub async fn check(&self, ip: &str) -> AppResult<()> {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap();
        let entries = state.entry(ip.to_string()).or_default();
        entries.retain(|t| now.duration_since(*t) < self.window);
        if entries.len() >= self.limit as usize {
            Err(AppError::RateLimited)
        } else {
            Ok(())
        }
    }

    pub async fn record_failure(&self, ip: &str) {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap();
        let entries = state.entry(ip.to_string()).or_default();
        entries.retain(|t| now.duration_since(*t) < self.window);
        entries.push(now);
    }

    pub async fn reset(&self, ip: &str) {
        self.state.lock().unwrap().remove(ip);
    }
}
