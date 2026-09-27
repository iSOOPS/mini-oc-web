use mini_oc_web::proxy::DeviceClient;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
