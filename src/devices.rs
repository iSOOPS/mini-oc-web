use crate::error::{AppError, AppResult};
use crate::sb::SbClient;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::Duration;

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

/// Result of probing one device's `GET /status` endpoint via the user's
/// cloud tunnel. Never throws — failures are encoded as
/// `online=false` with a `reason` string.
#[derive(Debug, Clone)]
pub struct DeviceProbeResult {
    pub online: bool,
    pub reason: Option<String>,
    pub status: Option<serde_json::Value>,
}

/// Probe `{cloud_ip}:{port}/status` with a 4s per-request timeout. Empty
/// credentials — the BFF→device health probe is credential-free by
/// design (ADR #13). Any failure (connection refused, timeout, non-2xx,
/// body parse error) is captured as `online=false` with a `reason`.
pub async fn probe_device_status(cloud_ip: &str, port: u16) -> DeviceProbeResult {
    let url = format!("http://{cloud_ip}:{port}/status");
    let http = match reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return DeviceProbeResult {
                online: false,
                reason: Some(format!("client build: {e}")),
                status: None,
            }
        }
    };
    match http.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => match resp.json::<serde_json::Value>().await {
            Ok(s) => DeviceProbeResult { online: true, reason: None, status: Some(s) },
            Err(e) => DeviceProbeResult {
                online: false,
                reason: Some(format!("body parse: {e}")),
                status: None,
            },
        },
        Ok(resp) => DeviceProbeResult {
            online: false,
            reason: Some(format!("device returned HTTP {}", resp.status())),
            status: None,
        },
        Err(e) => DeviceProbeResult {
            online: false,
            reason: Some(e.to_string()),
            status: None,
        },
    }
}

/// Thin store wrapping `SbClient` directly — no in-process TTL. Each
/// `load()` hits SB fresh (ADR #21).
pub struct DevicesStore {
    sb: std::sync::Arc<SbClient>,
    sb_user: String,
}

impl DevicesStore {
    pub fn new(sb: std::sync::Arc<SbClient>, sb_user: impl Into<String>) -> Self {
        Self {
            sb,
            sb_user: sb_user.into(),
        }
    }

    /// Load `serv/opencode/{user}/devices.json`. Missing file → empty
    /// registry (no end-side registrations yet). Always hits SB.
    pub async fn load(&self) -> AppResult<DevicesFile> {
        let path = DEVICES_PATH.replace("{user}", &self.sb_user);
        let body = match self.sb.get_fs(&path).await {
            Ok(b) => b,
            Err(AppError::NotFound(_)) => {
                return Ok(DevicesFile { version: 1, devices: Vec::new() });
            }
            Err(e) => return Err(e),
        };
        serde_json::from_str(&body)
            .map_err(|e| AppError::Internal(format!("devices.json parse: {}", e)))
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

#[cfg(test)]
mod store_tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn devices_store_load_hits_sb_every_call() {
        let sb = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/.fs/serv/opencode/admin/devices.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"version":1,"devices":[]}"#))
            .expect(2)
            .mount(&sb)
            .await;
        let store = DevicesStore::new(
            std::sync::Arc::new(crate::sb::SbClient::new(sb.uri(), "admin", "pw").unwrap()),
            "admin",
        );
        let _ = store.load().await.unwrap();
        let _ = store.load().await.unwrap();
    }

    #[tokio::test]
    async fn probe_device_status_returns_offline_when_unreachable() {
        // Probe a port nothing listens on → must return online=false within ~5s
        let result = probe_device_status("127.0.0.1", 1).await; // port 1 = unreachable
        assert!(!result.online, "expected offline, got: {:?}", result);
        assert!(result.reason.is_some());
    }
}