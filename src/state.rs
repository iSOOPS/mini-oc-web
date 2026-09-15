use crate::auth::RateLimiter;
use crate::devices::DevicesStore;
use crate::sb::SbClient;
use crate::users::UsersStore;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Per-device Basic-Auth credentials supplied by the user through the SPA
/// "device authorization" dialog (device key `pctype/pcname` → creds).
///
/// In-memory only, never persisted: a BFF restart clears it and the user
/// re-enters credentials on the next device visit. Entries take priority
/// over the unified `OPENCODE_SERVER_USERNAME/PASSWORD` from the config,
/// so a device whose credentials differ from the portal's keeps working
/// after the user authorizes once.
#[derive(Default)]
pub struct DeviceCreds {
    map: Mutex<HashMap<String, (String, String)>>,
}

impl DeviceCreds {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, device_key: &str) -> Option<(String, String)> {
        self.map.lock().unwrap().get(device_key).cloned()
    }

    pub fn set(&self, device_key: &str, username: &str, password: &str) {
        self.map
            .lock()
            .unwrap()
            .insert(device_key.to_string(), (username.to_string(), password.to_string()));
    }
}

#[derive(Clone)]
pub struct AppConfig {
    pub web_port: u16,
    pub web_bind: String,
    pub web_static_dir: String,
    /// Unified fallback credentials for the BFF→device API hop
    /// (OPENCODE_SERVER_USERNAME/PASSWORD). NOT used for portal login any
    /// more — portal users log in with per-user 32-char keys.
    pub device_user: String,
    pub device_pass: String,
    pub sb_user: String,
    pub sb_base_url: String,
    /// The SilverBullet password from env — doubles as the super-admin
    /// login password for /api/admin/login.
    pub sb_password: String,
    /// Rathole (cloud tunnel) key from env — surfaced to the admin page
    /// as "云服务密钥".
    pub rathole_key: String,
    pub cookie_key: Vec<u8>,
    /// The portal host the user reaches us on. Used to build deep-link
    /// URLs of the form `{portal_base}/{b64pc}/...`. Defaults to
    /// `https://oc.isoops.com`; override via `PORTAL_BASE`.
    pub portal_base: String,
}

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub sb: Arc<SbClient>,
    pub devices: Arc<DevicesStore>,
    pub users: Arc<UsersStore>,
    pub rate_limiter: Arc<RateLimiter>,
    pub device_creds: Arc<DeviceCreds>,
    /// Process start marker for /api/admin/info uptime.
    pub started_at: Instant,
}

/// Read an env var and strip trailing CR/LF (Windows-edited .env files
/// frequently end every line with CRLF; `dotenvy` keeps them, which makes
/// passwords and URLs silently mismatch).
fn env_var_trim(key: &str) -> Result<String, anyhow::Error> {
    Ok(std::env::var(key)?.trim_end_matches(['\r', '\n']).to_string())
}

impl AppState {
    pub async fn from_env() -> Result<Self, anyhow::Error> {
        let _ = dotenvy::dotenv();
        let web_port: u16 = std::env::var("WEB_PORT")
            .unwrap_or_else(|_| "8100".into())
            .parse()?;
        let web_bind = env_var_trim("WEB_BIND").unwrap_or_else(|_| "0.0.0.0".into());
        let web_static_dir =
            env_var_trim("WEB_STATIC_DIR").unwrap_or_else(|_| "./web/dist".into());
        let username = env_var_trim("OPENCODE_SERVER_USERNAME")?;
        let password = env_var_trim("OPENCODE_SERVER_PASSWORD")?;
        let sb_user = env_var_trim("SB_USER").unwrap_or_else(|_| "admin".into());
        let sb_password = env_var_trim("SB_PASSWORD")?;
        let sb_base_url =
            env_var_trim("SB_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into());
        let rathole_key = env_var_trim("RATHOLE_KEY").unwrap_or_default();

        let cookie_key_path =
            std::env::var("COOKIE_KEY_PATH").unwrap_or_else(|_| "/data/cookie_key".into());
        let cookie_key = load_or_create_key(&cookie_key_path)?;

        let sb = Arc::new(SbClient::new(sb_base_url.clone(), sb_user.clone(), sb_password.clone())?);
        sb.login().await?;
        let devices = Arc::new(DevicesStore::new(sb.clone(), sb_user.clone()));
        let users = Arc::new(UsersStore::new(sb.clone(), sb_user.clone()));
        let rate_limiter = Arc::new(RateLimiter::new(5, Duration::from_secs(600)));

        let portal_base = std::env::var("PORTAL_BASE")
            .unwrap_or_else(|_| "https://oc.isoops.com".into())
            .trim_end_matches('/')
            .to_string();

        Ok(Self {
            config: AppConfig {
                web_port,
                web_bind,
                web_static_dir,
                device_user: username,
                device_pass: password,
                sb_user,
                sb_base_url,
                sb_password,
                rathole_key,
                cookie_key,
                portal_base,
            },
            sb,
            devices,
            users,
            rate_limiter,
            device_creds: Arc::new(DeviceCreds::new()),
            started_at: Instant::now(),
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
