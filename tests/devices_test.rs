use mini_oc_web::devices::{Device, DevicesFile};
use serde_json::json;

#[test]
fn parses_valid_devices_json() {
    let raw = json!({
        "version": 1,
        "devices": [
            {
                "pctype": "macos",
                "pcname": "samuel",
                "public_url": "https://oc-mac.isoops.com",
                "oc_serve_port": 9464,
                "reported_at": "2026-08-28T10:00:00+08:00",
                "version": "0.1.0"
            }
        ]
    })
    .to_string();
    let parsed: DevicesFile = serde_json::from_str(&raw).unwrap();
    assert_eq!(parsed.version, 1);
    assert_eq!(parsed.devices.len(), 1);
    assert_eq!(parsed.devices[0].pctype, "macos");
    assert_eq!(parsed.devices[0].pcname, "samuel");
    assert_eq!(parsed.devices[0].public_url, "https://oc-mac.isoops.com");
    assert_eq!(parsed.devices[0].oc_serve_port, 9464);
}

#[test]
fn device_key_is_pctype_slash_pcname() {
    let d = Device {
        pctype: "windows".to_string(),
        pcname: "YG-PC".to_string(),
        public_url: "https://oc-win.isoops.com".to_string(),
        oc_serve_port: 9464,
        reported_at: None,
        version: None,
    };
    assert_eq!(d.key(), "windows/YG-PC");
}

#[test]
fn serializes_and_round_trips_back() {
    let original = DevicesFile {
        version: 1,
        devices: vec![Device {
            pctype: "macos".to_string(),
            pcname: "samuel".to_string(),
            public_url: "https://oc-mac.isoops.com".to_string(),
            oc_serve_port: 9464,
            reported_at: Some("2026-08-28T10:00:00+08:00".to_string()),
            version: Some("0.1.0".to_string()),
        }],
    };
    let json = serde_json::to_string(&original).unwrap();
    let parsed: DevicesFile = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, original);
}