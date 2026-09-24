use std::time::Duration;

/// Result of probing one device's `GET /status` endpoint. Never throws —
/// failures are encoded as `online=false` with a `reason` string.
#[derive(Debug, Clone)]
pub struct DeviceProbeResult {
    pub online: bool,
    pub reason: Option<String>,
    pub status: Option<serde_json::Value>,
}

impl DeviceProbeResult {
    /// An offline result the BFF produces without any network hop — for
    /// entries with no admin-assigned device URL (`public_url` empty):
    /// probing would have no target.
    pub fn offline(reason: impl Into<String>) -> Self {
        Self {
            online: false,
            reason: Some(reason.into()),
            status: None,
        }
    }
}

/// Probe a device's `GET <url>/status` (caller-supplied, taken from
/// `UserDevice.public_url`). 4s per-request timeout, empty credentials —
/// the BFF→device health probe is credential-free by design (ADR #13).
/// Any failure (connection refused, timeout, non-2xx, body parse error)
/// is captured as `online=false` with a `reason`.
pub async fn probe_device_status(url: String) -> DeviceProbeResult {
    let target = format!("{}/status", url.trim_end_matches('/'));
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
    match http.get(&target).send().await {
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

/// Cross-check a probe answer against the device entry it is about to be
/// attributed to. Several registry entries can resolve to the SAME probe
/// URL (entries sharing one admin-assigned `public_url`), so the
/// single device answering that URL would paint every entry with its own
/// runtime state — starting opencode on it turns all bound entries fully
/// available even though the other devices are offline.
///
/// The `/status` body carries the responder's platform in `system.os`
/// (`"macos"` / `"windows"` — same enum as `UserDevice.pctype`). When both
/// sides declare a known platform and they disagree, the answer came from
/// a different device: mark this entry `online=false` with an explanatory
/// reason and drop the foreign status snapshot. Empty/unknown `pctype`
/// (legacy data) and answers without `system.os` keep previous behavior.
pub fn verify_device_identity(pctype: &str, probe: DeviceProbeResult) -> DeviceProbeResult {
    if !probe.online {
        return probe;
    }
    let Some(os) = probe
        .status
        .as_ref()
        .and_then(|s| s.get("system"))
        .and_then(|sys| sys.get("os"))
        .and_then(|v| v.as_str())
        .map(str::to_ascii_lowercase)
    else {
        return probe;
    };
    let expected = pctype.trim().to_ascii_lowercase();
    let known = |v: &str| v == "macos" || v == "windows";
    if known(&expected) && known(&os) && expected != os {
        return DeviceProbeResult {
            online: false,
            reason: Some(format!(
                "probe target reports platform '{os}' but this device is '{expected}' — \
                 another device likely answers at this address"
            )),
            status: None,
        };
    }
    probe
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn probe_device_status_returns_offline_when_unreachable() {
        let result = probe_device_status("http://127.0.0.1:1".to_string()).await;
        assert!(!result.online, "expected offline, got: {:?}", result);
        assert!(result.reason.is_some());
    }

    #[test]
    fn verify_device_identity_flags_platform_mismatch() {
        let probe = DeviceProbeResult {
            online: true,
            reason: None,
            status: Some(serde_json::json!({
                "opencode_serve": "running",
                "system": {"os": "macos"},
            })),
        };
        let checked = verify_device_identity("windows", probe);
        assert!(!checked.online);
        assert!(
            checked.reason.unwrap().contains("platform"),
            "reason should explain the mismatch"
        );
        assert!(checked.status.is_none(), "foreign status snapshot dropped");
    }

    #[test]
    fn verify_device_identity_keeps_matching_and_unverifiable_answers() {
        let ok = DeviceProbeResult {
            online: true,
            reason: None,
            status: Some(serde_json::json!({"system": {"os": "windows"}})),
        };
        assert!(verify_device_identity("windows", ok).online);

        let no_os = DeviceProbeResult {
            online: true,
            reason: None,
            status: Some(serde_json::json!({"opencode_serve": "running"})),
        };
        assert!(verify_device_identity("windows", no_os).online);

        let legacy = DeviceProbeResult {
            online: true,
            reason: None,
            status: Some(serde_json::json!({"system": {"os": "macos"}})),
        };
        assert!(verify_device_identity("", legacy).online);

        let offline = DeviceProbeResult {
            online: false,
            reason: None,
            status: None,
        };
        assert!(!verify_device_identity("windows", offline).online);
    }
}
