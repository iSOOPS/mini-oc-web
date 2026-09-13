use mini_oc_web::proxy::DeviceClient;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn basic_auth_header() -> &'static str {
    "Basic b3BlbmNvZGU6Y2hhbmdlbWU="
}

#[tokio::test]
async fn health_returns_true_on_200() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    let client = DeviceClient::new(server.uri(), "opencode", "changeme");
    assert!(client.health().await.unwrap());
}

#[tokio::test]
async fn health_returns_false_on_500() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let client = DeviceClient::new(server.uri(), "opencode", "changeme");
    assert!(!client.health().await.unwrap());
}

#[tokio::test]
async fn projects_parses_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/project"))
        .and(wiremock::matchers::header("authorization", basic_auth_header()))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"[{"path":"/Users/samuel/projects/foo","lastOpenedAt":"2026-08-28T10:00:00+08:00"}]"#,
        ))
        .mount(&server)
        .await;
    let client = DeviceClient::new(server.uri(), "opencode", "changeme");
    let projects = client.projects().await.unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].path, "/Users/samuel/projects/foo");
}

/// Real `oc serve` wire format: worktree + ms timestamps (field probe
/// against 127.0.0.1:9464, see RawProject docs).
#[tokio::test]
async fn projects_parses_oc_serve_wire_format() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/project"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"[{"id":"global","worktree":"/","time":{"created":1784163362673,"updated":1789219158527},"sandboxes":[]}]"#,
        ))
        .mount(&server)
        .await;
    let client = DeviceClient::new(server.uri(), "opencode", "changeme");
    let projects = client.projects().await.unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].path, "/");
    assert!(projects[0].last_opened_at.as_deref().is_some_and(|s| s.starts_with("2026-")));
}

#[tokio::test]
async fn projects_returns_502_on_401() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/project"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let client = DeviceClient::new(server.uri(), "opencode", "changeme");
    let result = client.projects().await;
    assert!(matches!(result, Err(mini_oc_web::error::AppError::DeviceAuthFailed(_))));
}

#[tokio::test]
async fn create_session_sends_location_and_parses_wrapped_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/session"))
        .and(wiremock::matchers::body_partial_json(serde_json::json!({
            "location": { "directory": "/Users/samuel/projects/foo" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"data":{"id":"ses_new","title":"New session","location":{"directory":"/Users/samuel/projects/foo"},"time":{"created":1789229091321,"updated":1789229091321}}}"#,
        ))
        .mount(&server)
        .await;
    let client = DeviceClient::new(server.uri(), "opencode", "changeme");
    let created = client
        .create_session("/Users/samuel/projects/foo", None)
        .await
        .unwrap();
    assert_eq!(created.id, "ses_new");
    assert_eq!(created.directory.as_deref(), Some("/Users/samuel/projects/foo"));
}

#[tokio::test]
async fn create_session_parses_bare_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/session"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"id":"ses_new","title":"t","directory":"/"}"#,
        ))
        .mount(&server)
        .await;
    let client = DeviceClient::new(server.uri(), "opencode", "changeme");
    let created = client.create_session("/", Some("t")).await.unwrap();
    assert_eq!(created.id, "ses_new");
    assert_eq!(created.directory.as_deref(), Some("/"));
}