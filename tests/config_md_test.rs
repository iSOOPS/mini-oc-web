use mini_oc_web::config_md::{
    validate_pcname, default_config, PortalConfig, JumpOverride, ManualDevice,
};

#[test]
fn pcname_accepts_alphanumeric_dash_underscore() {
    for s in ["samuel", "samuel-mac", "YG-PC", "user_1", "a", "A".repeat(64).as_str()] {
        assert!(validate_pcname(s).is_ok(), "should accept: {}", s);
    }
}

#[test]
fn pcname_rejects_invalid() {
    for s in ["", "../etc", "foo bar", "foo/bar", "a".repeat(65).as_str(), "foo!", "føø"] {
        assert!(validate_pcname(s).is_err(), "should reject: {}", s);
    }
}

#[test]
fn default_config_is_empty() {
    let cfg = default_config("test-pc");
    assert_eq!(cfg.pcname, "test-pc");
    assert!(cfg.jump_overrides.is_empty());
    assert_eq!(cfg.default_base_url, "");
    assert!(cfg.manual_devices.is_empty());
    assert_eq!(cfg.version, 1);
}

#[test]
fn config_roundtrips_json() {
    let cfg = PortalConfig {
        version: 1,
        pcname: "samuel-mac".to_string(),
        updated_at: "2026-08-28T11:00:00+08:00".to_string(),
        jump_overrides: vec![JumpOverride {
            device: "macos/samuel".to_string(),
            base_url: "http://192.168.1.5:9464".to_string(),
        }],
        default_base_url: "".to_string(),
        manual_devices: vec![ManualDevice {
            id: "local-test".to_string(), label: "本机测试".to_string(),
            base_url: "http://127.0.0.1:9464".to_string(),
        }],
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: PortalConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.pcname, cfg.pcname);
    assert_eq!(parsed.jump_overrides.len(), 1);
    assert_eq!(parsed.manual_devices.len(), 1);
}