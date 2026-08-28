use mini_oc_web::error::{AppError, ErrorCode};

#[test]
fn error_serializes_to_unified_shape() {
    let err = AppError::Unauthorized("bad credentials".into());
    let json = serde_json::to_value(&err).unwrap();
    assert_eq!(json["error"]["code"], "unauthorized");
    assert_eq!(json["error"]["message"], "unauthorized: bad credentials");
}

#[test]
fn error_codes_match_design_doc() {
    assert_eq!(ErrorCode::Unauthorized.as_str(), "unauthorized");
    assert_eq!(ErrorCode::InvalidPcname.as_str(), "invalid_pcname");
    assert_eq!(ErrorCode::InvalidTarget.as_str(), "invalid_target");
    assert_eq!(ErrorCode::DeviceAuthFailed.as_str(), "device_auth_failed");
    assert_eq!(ErrorCode::DeviceOffline.as_str(), "device_offline");
    assert_eq!(ErrorCode::NotFound.as_str(), "not_found");
    assert_eq!(ErrorCode::RateLimited.as_str(), "rate_limited");
    assert_eq!(ErrorCode::Internal.as_str(), "internal");
    assert_eq!(ErrorCode::ServiceUnavailable.as_str(), "service_unavailable");
}

#[test]
fn error_status_codes_match_spec() {
    assert_eq!(AppError::Unauthorized("x".into()).status(), 401);
    assert_eq!(AppError::InvalidPcname("x".into()).status(), 400);
    assert_eq!(AppError::InvalidTarget("x".into()).status(), 400);
    assert_eq!(AppError::DeviceAuthFailed("x".into()).status(), 502);
    assert_eq!(AppError::DeviceOffline("x".into()).status(), 502);
    assert_eq!(AppError::NotFound("x".into()).status(), 404);
    assert_eq!(AppError::RateLimited.status(), 429);
    assert_eq!(AppError::Internal("x".into()).status(), 500);
    assert_eq!(AppError::ServiceUnavailable("x".into()).status(), 503);
}

#[test]
fn service_unavailable_serializes_correctly() {
    let err = AppError::ServiceUnavailable("SB unreachable".into());
    let json = serde_json::to_value(&err).unwrap();
    assert_eq!(json["error"]["code"], "service_unavailable");
    assert_eq!(
        json["error"]["message"],
        "service unavailable: SB unreachable"
    );
}
