use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Unauthorized(String),
    #[error("invalid pcname: {0}")]
    InvalidPcname(String),
    #[error("invalid target: {0}")]
    InvalidTarget(String),
    #[error("device auth failed: {0}")]
    DeviceAuthFailed(String),
    #[error("device offline: {0}")]
    DeviceOffline(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("rate limited")]
    RateLimited,
    #[error("internal: {0}")]
    Internal(String),
}

#[derive(Debug, Clone, Copy)]
pub enum ErrorCode {
    Unauthorized,
    InvalidPcname,
    InvalidTarget,
    DeviceAuthFailed,
    DeviceOffline,
    NotFound,
    RateLimited,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Unauthorized => "unauthorized",
            ErrorCode::InvalidPcname => "invalid_pcname",
            ErrorCode::InvalidTarget => "invalid_target",
            ErrorCode::DeviceAuthFailed => "device_auth_failed",
            ErrorCode::DeviceOffline => "device_offline",
            ErrorCode::NotFound => "not_found",
            ErrorCode::RateLimited => "rate_limited",
            ErrorCode::Internal => "internal",
        }
    }
}

impl AppError {
    pub fn code(&self) -> ErrorCode {
        match self {
            AppError::Unauthorized(_) => ErrorCode::Unauthorized,
            AppError::InvalidPcname(_) => ErrorCode::InvalidPcname,
            AppError::InvalidTarget(_) => ErrorCode::InvalidTarget,
            AppError::DeviceAuthFailed(_) => ErrorCode::DeviceAuthFailed,
            AppError::DeviceOffline(_) => ErrorCode::DeviceOffline,
            AppError::NotFound(_) => ErrorCode::NotFound,
            AppError::RateLimited => ErrorCode::RateLimited,
            AppError::Internal(_) => ErrorCode::Internal,
        }
    }

    pub fn status(&self) -> u16 {
        match self {
            AppError::Unauthorized(_) => 401,
            AppError::InvalidPcname(_) | AppError::InvalidTarget(_) => 400,
            AppError::DeviceAuthFailed(_) | AppError::DeviceOffline(_) => 502,
            AppError::NotFound(_) => 404,
            AppError::RateLimited => 429,
            AppError::Internal(_) => 500,
        }
    }
}

#[derive(Serialize)]
pub struct ErrorBody<'a> {
    pub error: ErrorDetail<'a>,
}

#[derive(Serialize)]
pub struct ErrorDetail<'a> {
    pub code: &'a str,
    pub message: String,
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ErrorBody {
            error: ErrorDetail {
                code: self.code().as_str(),
                message: self.to_string(),
            },
        }
        .serialize(serializer)
    }
}

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let status = axum::http::StatusCode::from_u16(self.status()).unwrap();
        let mut resp = (status, axum::Json(self)).into_response();
        resp.headers_mut()
            .insert("content-type", "application/json".parse().unwrap());
        resp
    }
}

pub type AppResult<T> = Result<T, AppError>;
