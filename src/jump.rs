use crate::error::{AppError, AppResult};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};

const FRAGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`');

pub fn build_jump_url(base_url: &str, directory: &str, session_id: &str) -> String {
    let encoded_dir = utf8_percent_encode(directory, FRAGMENT);
    let b64 = URL_SAFE_NO_PAD.encode(encoded_dir.to_string().as_bytes());
    format!("{}/{}/session/{}", base_url.trim_end_matches('/'), b64, session_id)
}

#[derive(Debug, Clone)]
pub struct JumpInput {
    pub device_key: String,
    pub override_url: Option<String>,
    pub default_base_url: Option<String>,
    pub devices_public_url: Option<String>,
}

pub fn resolve_base_url(input: &JumpInput) -> AppResult<String> {
    if let Some(ref override_url) = input.override_url {
        return validate_base_url(override_url);
    }
    if let Some(ref default_base_url) = input.default_base_url {
        if !default_base_url.is_empty() {
            if let Ok(url) = validate_base_url(default_base_url) {
                return Ok(url);
            }
        }
    }
    if let Some(ref devices_public_url) = input.devices_public_url {
        return validate_base_url(devices_public_url);
    }
    Err(AppError::InvalidTarget(format!(
        "no valid base_url for device {}",
        input.device_key
    )))
}

pub fn validate_base_url(s: &str) -> AppResult<String> {
    let url = url::Url::parse(s).map_err(|_| AppError::InvalidTarget(format!("invalid url: {}", s)))?;
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(AppError::InvalidTarget(format!("bad scheme: {}", s)));
    }
    if url.host_str().is_none() || url.host_str() == Some("") {
        return Err(AppError::InvalidTarget(format!("missing host: {}", s)));
    }
    if url.path() != "/" && !url.path().is_empty() {
        return Err(AppError::InvalidTarget(format!("path not allowed: {}", s)));
    }
    Ok(url.to_string().trim_end_matches('/').to_string())
}