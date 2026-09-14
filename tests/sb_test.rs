use mini_oc_web::sb::SbClient;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn login_stores_cookie() {
    let server = MockServer::start().await;
    // derive_cookie_name for 127.0.0.1:<port> = auth_127_0_0_1_<port>
    let expected_name = SbClient::new(server.uri(), "_", "_")
        .map(|c| c.cookie_name().to_string())
        .unwrap();
    Mock::given(method("POST"))
        .and(path("/.auth"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("ok")
                .append_header("set-cookie", format!("{expected_name}=jwt-token-xyz; Path=/")),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = SbClient::new(server.uri(), "admin", "secret").unwrap();
    client.login().await.unwrap();
    let cookie = client.cookie().expect("cookie must be set");
    assert!(
        cookie.starts_with(&expected_name),
        "cookie should be named {expected_name}, got {cookie}"
    );
}

#[tokio::test]
async fn get_fs_returns_body() {
    let server = MockServer::start().await;
    let expected_name = SbClient::new(server.uri(), "_", "_")
        .map(|c| c.cookie_name().to_string())
        .unwrap();
    Mock::given(method("POST"))
        .and(path("/.auth"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("ok")
                .append_header("set-cookie", format!("{expected_name}=jwt-token-xyz; Path=/")),
        )
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
    let expected_name = SbClient::new(server.uri(), "_", "_")
        .map(|c| c.cookie_name().to_string())
        .unwrap();
    Mock::given(method("POST"))
        .and(path("/.auth"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("ok")
                .append_header("set-cookie", format!("{expected_name}=jwt-token-xyz; Path=/")),
        )
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

/// 401 from `/.fs/*` should trigger an auto-relogin + retry (transparently).
#[tokio::test]
async fn get_fs_auto_relogin_on_401() {
    let server = MockServer::start().await;
    let expected_name = SbClient::new(server.uri(), "_", "_")
        .map(|c| c.cookie_name().to_string())
        .unwrap();
    // Two login calls: the initial one + the auto-relogin after 401.
    Mock::given(method("POST"))
        .and(path("/.auth"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("ok")
                .append_header("set-cookie", format!("{expected_name}=jwt-token-xyz; Path=/")),
        )
        .expect(2)
        .mount(&server)
        .await;
    // First GET returns 401, second returns 200. wiremock serves in registration
    // order so first call gets 401, then 200.
    Mock::given(method("GET"))
        .and(path("/.fs/test"))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/.fs/test"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok-after-relogin"))
        .mount(&server)
        .await;

    let client = SbClient::new(server.uri(), "admin", "secret").unwrap();
    client.login().await.unwrap();
    let body = client.get_fs("test").await.unwrap();
    assert_eq!(body, "ok-after-relogin");
}