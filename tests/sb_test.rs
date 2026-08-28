use mini_oc_web::sb::SbClient;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn login_stores_cookie() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"<!doctype html><html><body>OK</body></html>"#)
                .append_header("set-cookie", "sid=abc123; Path=/"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = SbClient::new(server.uri(), "admin", "secret").unwrap();
    client.login().await.unwrap();
    assert!(client.cookie().is_some());
}

#[tokio::test]
async fn get_fs_returns_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/.fs/serv/web/test-pc/config.md"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"version":1}"#))
        .expect(1)
        .mount(&server)
        .await;

    let client = SbClient::new(server.uri(), "admin", "secret").unwrap();
    client.login().await.unwrap();
    let body = client.get_fs("serv/web/test-pc/config.md").await.unwrap();
    assert_eq!(body, r#"{"version":1}"#);
}

#[tokio::test]
async fn put_fs_sends_body_with_cookie() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/.fs/serv/web/test-pc/config.md"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let client = SbClient::new(server.uri(), "admin", "secret").unwrap();
    client.login().await.unwrap();
    client
        .put_fs("serv/web/test-pc/config.md", r#"{"version":1}"#)
        .await
        .unwrap();
}
