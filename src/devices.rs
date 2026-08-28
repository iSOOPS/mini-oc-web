use crate::error::{AppError, AppResult};
use crate::sb::SbClient;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const DEVICES_PATH: &str = "serv/opencode/{user}/devices.json";

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