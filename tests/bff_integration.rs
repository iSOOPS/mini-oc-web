use axum::http::StatusCode;
use mini_oc_web::routes::build_router;
use mini_oc_web::state::AppState;
use std::path::PathBuf;
use std::sync::Arc;

mod minid {
    pub use mini_oc_web::devices::*;
}

fn temp_static_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mini-oc-web-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "<html>test-spa</html>").unwrap();
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(
        dir.join("assets").join("index-test.js"),
        "console.log('test');",
    )
    .unwrap();
    dir
}

fn test_app_config(static_dir: PathBuf) -> mini_oc_web::state::AppConfig {
    mini_oc_web::state::AppConfig {
        web_port: 0,
        web_bind: "127.0.0.1".to_string(),
        web_static_dir: static_dir.to_string_lossy().to_string(),
        username: "testuser".to_string(),
        password: "testpass".to_string(),
        sb_user: "admin".to_string(),
        sb_base_url: "http://127.0.0.1:65535".to_string(),
        cookie_key: vec![0u8; 32],
    }
}

async fn spawn_test_server() -> (String, tokio::task::JoinHandle<()>) {
    let static_dir = temp_static_dir();
    let app_config = test_app_config(static_dir.clone());

    let sb = Arc::new(
        mini_oc_web::sb::SbClient::new(
            "http://127.0.0.1:65535",
            "admin",
            "x",
        )
        .unwrap(),
    );
    let devices = Arc::new(minid::DevicesCache::new(
        sb.clone(),
        "admin",
        std::time::Duration::from_secs(30),
    ));
    let rl = Arc::new(mini_oc_web::auth::RateLimiter::new(5, std::time::Duration::from_secs(600)));
    let state = AppState {
        config: app_config,
        sb,
        devices,
        rate_limiter: rl,
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_router(state);
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{}", addr), handle)
}

#[tokio::test]
async fn healthz_works() {
    let (base, _h) = spawn_test_server().await;
    let resp = reqwest::get(format!("{}/healthz", base)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.text().await.unwrap();
    assert_eq!(body, "ok\n");
}

#[tokio::test]
async fn spa_index_served() {
    let (base, _h) = spawn_test_server().await;
    let resp = reqwest::get(format!("{}/", base)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(ct.contains("text/html"));
    let body = resp.text().await.unwrap();
    assert!(body.contains("test-spa"));
}

#[tokio::test]
async fn spa_route_falls_back_to_index() {
    let (base, _h) = spawn_test_server().await;
    let resp = reqwest::get(format!("{}/devices", base)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.text().await.unwrap();
    assert!(body.contains("test-spa"));
}

#[tokio::test]
async fn assets_served_as_js() {
    let (base, _h) = spawn_test_server().await;
    let resp = reqwest::get(format!("{}/assets/index-test.js", base)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(ct.contains("javascript") || ct.contains("ecmascript"));
    let body = resp.text().await.unwrap();
    assert!(body.contains("test"));
}

#[tokio::test]
async fn login_bad_creds_returns_401() {
    let (base, _h) = spawn_test_server().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/login", base))
        .json(&serde_json::json!({"username":"x","password":"y"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "unauthorized");
}

#[tokio::test]
async fn login_good_creds_returns_cookie() {
    let (base, _h) = spawn_test_server().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/login", base))
        .json(&serde_json::json!({"username":"testuser","password":"testpass"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let cookie = resp.headers().get("set-cookie");
    assert!(cookie.is_some());
    let cookie_str = cookie.unwrap().to_str().unwrap();
    assert!(cookie_str.contains("mini_oc_web_session="));
}

#[tokio::test]
async fn devices_endpoint_returns_503_when_sb_unreachable() {
    let (base, _h) = spawn_test_server().await;
    let client = reqwest::Client::new();
    let login = client
        .post(format!("{}/api/login", base))
        .json(&serde_json::json!({"username":"testuser","password":"testpass"}))
        .send()
        .await
        .unwrap();
    let cookie = login
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let resp = client
        .get(format!("{}/api/devices", base))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "service_unavailable");
}

#[tokio::test]
async fn invalid_pcname_returns_400() {
    let (base, _h) = spawn_test_server().await;
    let client = reqwest::Client::new();
    let login = client
        .post(format!("{}/api/login", base))
        .json(&serde_json::json!({"username":"testuser","password":"testpass"}))
        .send()
        .await
        .unwrap();
    let cookie = login
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let resp = client
        .get(format!("{}/api/config?pcname=foo%2Fbar", base))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "invalid_pcname");
}

#[tokio::test]
async fn no_cookie_returns_401() {
    let (base, _h) = spawn_test_server().await;
    let resp = reqwest::get(format!("{}/api/devices", base)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
