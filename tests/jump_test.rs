use base64::Engine;
use mini_oc_web::jump::{
    append_auth_token, build_jump_url, resolve_base_url, validate_base_url, JumpInput,
};

#[test]
fn jump_url_ascii_path() {
    let url = build_jump_url(
        "https://oc-mac.isoops.com",
        "/Users/samuel/projects/foo",
        "ses_abc123",
    );
    assert_eq!(
        url,
        "https://oc-mac.isoops.com/L1VzZXJzL3NhbXVlbC9wcm9qZWN0cy9mb28/session/ses_abc123"
    );
}

#[test]
fn auth_token_appended_with_question_mark() {
    let url = append_auth_token("http://127.0.0.1:9464/Lw/session/ses_1", "opencode", "changeme");
    let expected = base64::engine::general_purpose::STANDARD.encode("opencode:changeme");
    assert_eq!(
        url,
        format!(
            "http://127.0.0.1:9464/Lw/session/ses_1?auth_token={}",
            urlencoding::encode(&expected)
        )
    );
}

#[test]
fn auth_token_appended_with_ampersand_when_query_exists() {
    let url = append_auth_token("http://h/p?x=1", "u", "p");
    assert!(url.starts_with("http://h/p?x=1&auth_token="), "got {}", url);
}

#[test]
fn auth_token_percent_encodes_base64_specials() {
    // "+/==" produced by standard base64 must be percent-encoded so
    // form-decoding on the device does not corrupt the token.
    let url = append_auth_token("http://h/", "user", "pass");
    let token_part = url.split("auth_token=").nth(1).unwrap();
    assert!(!token_part.contains('+') && !token_part.contains('/') && !token_part.contains('='), "got {}", token_part);
    // round-trips through percent-decoding back to standard base64
    let decoded = urlencoding::decode(token_part).unwrap().to_string();
    assert_eq!(
        decoded,
        base64::engine::general_purpose::STANDARD.encode("user:pass")
    );
}

#[test]
fn auth_token_skipped_for_empty_credentials() {
    assert_eq!(append_auth_token("http://h/p", "", ""), "http://h/p");
}


#[test]
fn jump_url_chinese_path_percent_encoded() {
    let url = build_jump_url(
        "https://oc-mac.isoops.com",
        "/Users/小明/学习",
        "ses_xyz",
    );
    assert!(url.starts_with("https://oc-mac.isoops.com/"));
    assert!(url.ends_with("/session/ses_xyz"));
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode("/Users/%E5%B0%8F%E6%98%8E/%E5%AD%A6%E4%B9%A0".as_bytes());
    assert!(url.contains(&b64), "url={} should contain b64={}", url, b64);
}

#[test]
fn validate_base_url_accepts_http_https() {
    assert!(validate_base_url("http://192.168.1.5:9464").is_ok());
    assert!(validate_base_url("https://oc-mac.isoops.com").is_ok());
    assert!(validate_base_url("http://127.0.0.1:9464").is_ok());
}

#[test]
fn validate_base_url_rejects_others() {
    assert!(validate_base_url("javascript:alert(1)").is_err());
    assert!(validate_base_url("data:text/html,foo").is_err());
    assert!(validate_base_url("file:///etc/passwd").is_err());
    assert!(validate_base_url("ftp://example.com").is_err());
    assert!(validate_base_url("").is_err());
    assert!(validate_base_url("not-a-url").is_err());
}

#[test]
fn validate_base_url_rejects_path() {
    assert!(validate_base_url("http://192.168.1.5:9464/foo").is_err());
    assert!(validate_base_url("https://oc-mac.isoops.com/api/health").is_err());
}

#[test]
fn resolve_base_url_priority_override_default_devices() {
    let input = JumpInput {
        device_key: "macos/samuel".to_string(),
        override_url: Some("http://192.168.1.5:9464".to_string()),
        default_base_url: Some("http://fallback.example.com:9464".to_string()),
        devices_public_url: Some("https://oc-mac.isoops.com".to_string()),
    };
    assert_eq!(
        resolve_base_url(&input).unwrap(),
        "http://192.168.1.5:9464"
    );
}

#[test]
fn resolve_base_url_priority_default_over_devices() {
    let input = JumpInput {
        device_key: "macos/samuel".to_string(),
        override_url: None,
        default_base_url: Some("http://fallback.example.com:9464".to_string()),
        devices_public_url: Some("https://oc-mac.isoops.com".to_string()),
    };
    assert_eq!(
        resolve_base_url(&input).unwrap(),
        "http://fallback.example.com:9464"
    );
}

#[test]
fn resolve_base_url_falls_back_to_devices_public_url() {
    let input = JumpInput {
        device_key: "macos/samuel".to_string(),
        override_url: None,
        default_base_url: None,
        devices_public_url: Some("https://oc-mac.isoops.com".to_string()),
    };
    assert_eq!(
        resolve_base_url(&input).unwrap(),
        "https://oc-mac.isoops.com"
    );
}

#[test]
fn resolve_base_url_errors_when_no_source() {
    let input = JumpInput {
        device_key: "macos/samuel".to_string(),
        override_url: None,
        default_base_url: None,
        devices_public_url: None,
    };
    assert!(resolve_base_url(&input).is_err());
}

#[test]
fn resolve_base_url_errors_when_override_is_malformed() {
    let input = JumpInput {
        device_key: "macos/samuel".to_string(),
        override_url: Some("javascript:alert(1)".to_string()),
        default_base_url: None,
        devices_public_url: Some("https://oc-mac.isoops.com".to_string()),
    };
    assert!(resolve_base_url(&input).is_err());
}