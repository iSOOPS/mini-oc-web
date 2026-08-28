use crate::error::{AppError, AppResult};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PortalConfig {
    pub version: u32,
    pub pcname: String,
    #[serde(rename = "updated_at")]
    pub updated_at: String,
    #[serde(rename = "jump_overrides", default)]
    pub jump_overrides: Vec<JumpOverride>,
    #[serde(rename = "default_base_url", default)]
    pub default_base_url: String,
    #[serde(rename = "manual_devices", default)]
    pub manual_devices: Vec<ManualDevice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JumpOverride {
    pub device: String,
    #[serde(rename = "base_url")]
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManualDevice {
    pub id: String,
    pub label: String,
    #[serde(rename = "base_url")]
    pub base_url: String,
}

pub fn default_config(pcname: &str) -> PortalConfig {
    PortalConfig {
        version: 1,
        pcname: pcname.to_string(),
        updated_at: String::new(),
        jump_overrides: Vec::new(),
        default_base_url: String::new(),
        manual_devices: Vec::new(),
    }
}

/// Whitelist: `[A-Za-z0-9_-]{1,64}` (design doc §7.2)
pub fn validate_pcname(s: &str) -> AppResult<()> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9_-]{1,64}$").unwrap());
    if re.is_match(s) {
        Ok(())
    } else {
        Err(AppError::InvalidPcname(s.to_string()))
    }
}
