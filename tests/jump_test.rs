use base64::Engine;
use mini_oc_web::jump::{append_auth_token, build_jump_url, pcname_b64};

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
    assert!(
        !token_part.contains('+') && !token_part.contains('/') && !token_part.contains('='),
        "got {}",
        url
    );
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
    let url = build_jump_url("https://oc-mac.isoops.com", "/Users/小明/学习", "ses_xyz");
    assert!(url.starts_with("https://oc-mac.isoops.com/"));
    assert!(url.ends_with("/session/ses_xyz"));
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode("/Users/%E5%B0%8F%E6%98%8E/%E5%AD%A6%E4%B9%A0".as_bytes());
    assert!(url.contains(&b64), "url={} should contain b64={}", url, b64);
}

#[test]
fn pcname_b64_matches_url_safe_no_pad() {
    assert_eq!(pcname_b64("samuel"), base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"samuel"));
}
