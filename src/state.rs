use crate::auth::RateLimiter;
use crate::config_md::PortalConfig;
use crate::devices::DevicesCache;
use crate::sb::SbClient;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct AppConfig {
    pub web_port: u16,
    pub web_bind: String,
    pub web_static_dir: String,
    pub username: String,
    pub password: String,
    pub sb_user: String,
    pub sb_base_url: String,
    pub cookie_key: Vec<u8>,
}

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub sb: Arc<SbClient>,
    pub devices: Arc<DevicesCache>,
    pub rate_limiter: Arc<RateLimiter>,
}

impl AppState {
    pub async fn from_env() -> Result<Self, anyhow::Error> {
        let _ = dotenvy::dotenv();
        let web_port: u16 = std::env::var("WEB_PORT")
            .unwrap_or_else(|_| "8100".into())
            .parse()?;
        let web_bind = std::env::var("WEB_BIND").unwrap_or_else(|_| "0.0.0.0".into());
        let web_static_dir =
            std::env::var("WEB_STATIC_DIR").unwrap_or_else(|_| "./web/dist".into());
        let username = std::env::var("OPENCODE_SERVER_USERNAME")?;
        let password = std::env::var("OPENCODE_SERVER_PASSWORD")?;
        let sb_user = std::env::var("SB_USER").unwrap_or_else(|_| "admin".into());
        let sb_password = std::env::var("SB_PASSWORD")?;
        let sb_base_url =
            std::env::var("SB_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into());

        let cookie_key_path =
            std::env::var("COOKIE_KEY_PATH").unwrap_or_else(|_| "/data/cookie_key".into());
        let cookie_key = load_or_create_key(&cookie_key_path)?;

        let sb = Arc::new(SbClient::new(sb_base_url.clone(), sb_user.clone(), sb_password)?);
        sb.login().await?;
        let devices = Arc::new(DevicesCache::new(sb.clone(), sb_user.clone(), Duration::from_secs(30)));
        let rate_limiter = Arc::new(RateLimiter::new(5, Duration::from_secs(600)));

        Ok(Self {
            config: AppConfig {
                web_port,
                web_bind,
                web_static_dir,
                username,
                password,
                sb_user,
                sb_base_url,
                cookie_key,
            },
            sb,
            devices,
            rate_limiter,
        })
    }
}

fn load_or_create_key(path: &str) -> anyhow::Result<Vec<u8>> {
        use std::io::Read;
    use std::io::Write;
    if let Ok(mut f) = std::fs::File::open(path) {
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        if buf.len() == 32 {
            return Ok(buf);
        }
    }
    use rand::RngCore;
    let mut buf = vec![0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::File::create(path)?;
    f.write_all(&buf)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(buf)
}

pub fn default_config_for(pcname: &str) -> PortalConfig {
    crate::config_md::default_config(pcname)
}