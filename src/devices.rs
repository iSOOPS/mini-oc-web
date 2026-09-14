use crate::error::{AppError, AppResult};
use crate::sb::SbClient;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const DEVICES_PATH: &str = "serv/opencode/{user}/devices.json";

/// `serv/opencode/{user_id}/{pctype}/{device_name}/path-list.md` — the
/// project/session registry the mini-oc-gui end-side maintains on the
/// SilverBullet store. Multi-tenant layout: `user_id` is the portal
/// user's id (NOT the admin SB_USER), while `pctype` and `device_name`
/// come from the user's device-list entry (设备清单) in
/// `web/opencode/config.md` — the same values the binding client
/// reported via POST /api/device-bind.
pub const PATH_LIST_PATH: &str = "serv/opencode/{user_id}/{pctype}/{device_name}/path-list.md";

/// Build the per-device path-list.md location for a portal user:
/// `user_id` identifies the tenant, `pctype`/`device_name` identify the
/// device as keyed by the user's device list (设备清单).
pub fn path_list_path(user_id: &str, pctype: &str, device_name: &str) -> String {
    PATH_LIST_PATH
        .replace("{user_id}", user_id)
        .replace("{pctype}", pctype)
        .replace("{device_name}", device_name)
}

/// One entry of `path-list.md`. Wire shape mirrors mini-oc-gui
/// `domain::PathEntry` (camelCase timestamps, verified against the live
/// SB document): `{"path":"D:/x","sections":["ses_…"],"createdAt":
/// "2026-09-09T23:25:10+08:00","lastOpenedAt":"2026-09-13T00:59:41+08:00"}`.
/// Timestamps are passed through as RFC-3339 strings.
#[derive(Debug, Clone, Deserialize)]
pub struct PathListEntry {
    pub path: String,
    #[serde(default)]
    pub sections: Vec<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default, rename = "lastOpenedAt")]
    pub last_opened_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DevicesFile {
    pub version: u32,
    pub devices: Vec<Device>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Device {
    pub pctype: String,
    pub pcname: String,
    #[serde(rename = "public_url")]
    pub public_url: String,
    #[serde(rename = "oc_serve_port")]
    pub oc_serve_port: u16,
    #[serde(rename = "reported_at", default, skip_serializing_if = "Option::is_none")]
    pub reported_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl Device {
    pub fn key(&self) -> String {
        format!("{}/{}", self.pctype, self.pcname)
    }
}

pub struct DevicesCache {
    sb: std::sync::Arc<SbClient>,
    sb_user: String,
    ttl: Duration,
    state: Mutex<Option<(Instant, DevicesFile)>>,
}

impl DevicesCache {
    pub fn new(sb: std::sync::Arc<SbClient>, sb_user: impl Into<String>, ttl: Duration) -> Self {
        Self {
            sb,
            sb_user: sb_user.into(),
            ttl,
            state: Mutex::new(None),
        }
    }

    pub async fn load(&self) -> AppResult<DevicesFile> {
        let now = Instant::now();
        {
            let state = self.state.lock().unwrap();
            if let Some((ts, df)) = state.as_ref() {
                if now.duration_since(*ts) < self.ttl {
                    return Ok(df.clone());
                }
            }
        }
        let path = DEVICES_PATH.replace("{user}", &self.sb_user);
        let body = match self.sb.get_fs(&path).await {
            Ok(b) => b,
            Err(AppError::NotFound(_)) => {
                let df = DevicesFile {
                    version: 1,
                    devices: Vec::new(),
                };
                *self.state.lock().unwrap() = Some((now, df.clone()));
                return Ok(df);
            }
            Err(AppError::ServiceUnavailable(msg)) => {
                return Err(AppError::ServiceUnavailable(format!(
                    "devices cache unavailable: {}",
                    msg
                )));
            }
            Err(e) => return Err(e),
        };
        let df: DevicesFile = serde_json::from_str(&body)
            .map_err(|e| AppError::Internal(format!("devices.json parse: {}", e)))?;
        *self.state.lock().unwrap() = Some((now, df.clone()));
        Ok(df)
    }

    pub async fn invalidate(&self) {
        self.state.lock().unwrap().take();
    }
}

/// Whitelist: `[A-Za-z0-9_-]{1,64}` (design doc §7.2). Validates device
/// names (`pcname`) everywhere; since the multi-tenant refactor also reused
/// for portal user names and their assigned device names (see `users.rs`).
pub fn validate_pcname(s: &str) -> AppResult<()> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9_-]{1,64}$").unwrap());
    if re.is_match(s) {
        Ok(())
    } else {
        Err(AppError::InvalidPcname(s.to_string()))
    }
}