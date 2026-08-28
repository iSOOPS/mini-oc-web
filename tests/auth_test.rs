use mini_oc_web::auth::{sign_cookie, verify_cookie, RateLimiter};
use std::sync::Arc;
use std::time::Duration;

#[test]
fn cookie_signs_and_verifies() {
    let key = b"0123456789abcdef0123456789abcdef";
    let cookie = sign_cookie("alice", key, Duration::from_secs(60)).unwrap();
    let verified = verify_cookie(&cookie, key).unwrap();
    assert_eq!(verified, "alice");
}

#[test]
fn cookie_rejects_tampering() {
    let key = b"0123456789abcdef0123456789abcdef";
    let cookie = sign_cookie("alice", key, Duration::from_secs(60)).unwrap();
    let mut tampered: String = cookie.clone();
    let mid = tampered.len() / 2;
    let bad_char = if tampered.as_bytes()[mid] == b'a' { 'b' } else { 'a' };
    tampered.replace_range(mid..mid + 1, &bad_char.to_string());
    assert!(verify_cookie(&tampered, key).is_err());
}

#[test]
fn cookie_rejects_expired() {
    let key = b"0123456789abcdef0123456789abcdef";
    let cookie = sign_cookie("alice", key, Duration::from_millis(0)).unwrap();
    std::thread::sleep(Duration::from_millis(10));
    assert!(verify_cookie(&cookie, key).is_err());
}

#[test]
fn cookie_rejects_wrong_key() {
    let key1 = b"0123456789abcdef0123456789abcdef";
    let key2 = b"fedcba9876543210fedcba9876543210";
    let cookie = sign_cookie("alice", key1, Duration::from_secs(60)).unwrap();
    assert!(verify_cookie(&cookie, key2).is_err());
}

#[tokio::test]
async fn rate_limiter_blocks_after_5_failures() {
    let limiter = Arc::new(RateLimiter::new(5, Duration::from_secs(600)));
    let ip = "1.2.3.4";
    for _ in 0..5 {
        assert!(limiter.check(ip).await.is_ok());
        limiter.record_failure(ip).await;
    }
    assert!(limiter.check(ip).await.is_err());
}

#[tokio::test]
async fn rate_limiter_is_per_ip() {
    let limiter = Arc::new(RateLimiter::new(2, Duration::from_secs(600)));
    for _ in 0..2 {
        limiter.record_failure("1.1.1.1").await;
    }
    assert!(limiter.check("1.1.1.1").await.is_err());
    assert!(limiter.check("2.2.2.2").await.is_ok());
}
