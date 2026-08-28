use mini_oc_web::proxy::{DeviceClient, ProjectInfo, SessionInfo};
use wiremock::matchers::{method, path, query_param};
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
async fn sessions_parses_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/session"))
        .and(query_param("directory", "/Users/samuel/projects/foo"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"[{"id":"ses_abc","title":"My Session","updatedAt":"2026-08-28T10:00:00+08:00"}]"#,
        ))
        .mount(&server)
        .await;
    let client = DeviceClient::new(server.uri(), "opencode", "changeme");
    let sessions = client
        .sessions("/Users/samuel/projects/foo")
        .await
        .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "ses_abc");
    assert_eq!(sessions[0].title, Some("My Session".to_string()));
}