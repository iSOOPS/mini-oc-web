//! Multi-tenant portal users, persisted as JSON inside the SilverBullet
//! document `web/opencode/config.md`.
//!
//! The file shape mirrors `devices.json`: a versioned wrapper plus a list.
//! The BFF (via the admin API) is the only writer; end-sides never touch
//! it. Reads hit SB directly on every call (no in-process cache).

use crate::error::{AppError, AppResult};
use crate::sb::SbClient;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};

/// SB path of the multi-tenant user registry (JSON body in a .md doc).
pub const USERS_PATH: &str = "web/opencode/config.md";

/// Alphabet for generated login keys: letters + digits only, no symbols
/// (keys must survive copy-paste, chat apps and manual typing).
const KEY_ALPHABET: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
const KEY_LEN: usize = 32;

/// One entry of a user's device list ("设备清单"): the human-readable
/// description (`desc`), the device's service name, the local TUI service
/// port (mini-oc-gui), the opencode server port the TUI launches, the
/// device name written by the binding client, the platform type, the
/// binding status, and the device's tunnel address (`public_url` —
/// admin-assigned `scheme://host` with NO port: the BFF reaches the TUI
/// at `{public_url}:{port}` and opencode at `{public_url}:{oc_port}`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UserDevice {
    /// Human-readable description (描述), display-only; 1-64 chars
    /// after trimming, no control characters. Renamed from the legacy
    /// `d-name` field (which still deserializes as an alias, so old
    /// registry entries keep loading and migrate on the next save).
    #[serde(rename = "desc")]
    pub desc: String,
    /// Device service name: lowercase letters/digits/hyphens only
    /// (`^[a-z0-9-]{1,64}$`) — it becomes path/URL material.
    pub name: String,
    /// 服务端口号：the local TUI service port (mini-oc-gui, device
    /// default 9465). The BFF probes `{public_url}:{port}/status` and
    /// targets `{public_url}:{port}` for the TUI's `/project`,
    /// `/api/session` endpoints.
    pub port: u16,
    /// OpenCode 端口号：the port the TUI launches `opencode serve` on
    /// (device default 9464). The BFF builds browser jump URLs as
    /// `{public_url}:{oc_port}/...`. Must differ from `port`.
    #[serde(rename = "oc-port", default = "default_oc_port")]
    pub oc_port: u16,
    /// 设备名称（device-name）：由设备客户端绑定时（POST
    /// /api/device-bind）自动写入；管理端不可编辑。Empty = not yet
    /// written by a binding client.
    #[serde(rename = "device-name", default)]
    pub device_name: String,
    /// 平台类型（pctype）：`windows` / `macos` — same field name and
    /// values as the platform-type path segment of the SB path-list
    /// (`serv/opencode/{user}/{pctype}/{pcname}/path-list.md`).
    #[serde(rename = "pctype", default)]
    pub pctype: String,
    /// Binding status (绑定状态): whether the device is bound to the
    /// user; defaults to false for legacy registry entries.
    #[serde(default)]
    pub bound: bool,
    /// 设备穿透地址（tunnel address）, admin-assigned as a bare
    /// `scheme://host` (IP or domain, http/https — NO port, NO path).
    /// The BFF reaches the TUI at `{public_url}:{port}` and opencode at
    /// `{public_url}:{oc_port}`. Empty (legacy registry entries only —
    /// new writes are validated non-empty) means the device cannot be
    /// probed and is reported offline with a "not configured" reason.
    #[serde(rename = "public-url", default)]
    pub public_url: String,
}

/// Default opencode server port — mirrors mini-oc-gui's
/// `DEFAULT_OPENCODE_PORT` (its TUI launches `opencode serve --port
/// 9464`). Legacy registry entries without an explicit `oc-port`
/// deserialize with this value.
fn default_oc_port() -> u16 {
    9464
}

/// Platform types a device entry may declare (mirrors the SB path-list
/// platform-type enum and `parse_device_path` in routes.rs).
pub const DEVICE_PCTYPES: [&str; 2] = ["windows", "macos"];

/// Port assumed for legacy device entries written before the {name, port}
/// schema ("设备清单" refactor). Admins can correct it per device on the
/// next edit; any save rewrites the registry in the new format.
pub const LEGACY_DEFAULT_PORT: u16 = 4040;

/// Backwards-compatible deserialization for `UserDevice`:
/// - new format: `{"desc": "办公本", "name": "dev-a", "port": 4040,
///   "device-name": "HOME-WIN", "pctype": "windows", "bound": true}`
///   (`desc` falls back to the legacy `d-name` alias when present, every
///   optional field falls back to empty/false — `port` falls back to the
///   legacy default)
/// - legacy format (pre-设备清单 registry): a bare `"dev-a"` string —
///   parsed with the legacy default port so existing
///   `web/opencode/config.md` documents keep loading.
impl<'de> Deserialize<'de> for UserDevice {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Full {
                #[serde(rename = "desc", alias = "d-name", default)]
                desc: String,
                name: String,
                #[serde(default = "legacy_default_port")]
                port: u16,
                #[serde(rename = "oc-port", default = "default_oc_port")]
                oc_port: u16,
                #[serde(rename = "device-name", default)]
                device_name: String,
                #[serde(rename = "pctype", default)]
                pctype: String,
                #[serde(default)]
                bound: bool,
                #[serde(rename = "public-url", default)]
                public_url: String,
            },
            Legacy(String),
        }
        fn legacy_default_port() -> u16 {
            LEGACY_DEFAULT_PORT
        }

        Ok(match Wire::deserialize(deserializer)? {
            Wire::Full {
                desc,
                name,
                port,
                oc_port,
                device_name,
                pctype,
                bound,
                public_url,
            } => UserDevice {
                desc,
                name,
                port,
                oc_port,
                device_name,
                pctype,
                bound,
                public_url,
            },
            Wire::Legacy(name) => UserDevice {
                desc: String::new(),
                name,
                port: LEGACY_DEFAULT_PORT,
                oc_port: default_oc_port(),
                device_name: String::new(),
                pctype: String::new(),
                bound: false,
                public_url: String::new(),
            },
        })
    }
}

/// One tenant of the portal. `devices` is the device list ("设备清单"):
/// devices whose `name` matches a `pcname` in devices.json the user may
/// see and operate; everything else is hidden and returns 403 on direct
/// API access.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PortalUser {
    pub id: String,
    pub name: String,
    /// 32-char random `[A-Za-z0-9]` login key (no symbols).
    pub key: String,
    /// Cloud-service IP the user's devices tunnel through (rathole
    /// server). Editable by the user; defaults to the fleet default.
    #[serde(default = "default_cloud_ip")]
    pub cloud_ip: String,
    /// Last successful login (RFC-3339), stamped by the BFF.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
    #[serde(default)]
    pub devices: Vec<UserDevice>,
    /// 用户级 SilverBullet 连接配置（域名/账号/密码）：用户设置页可改，
    /// 存储在本注册表文档中；域名默认 fleet 常量。
    #[serde(default)]
    pub sb: UserSbConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Fleet-default cloud (rathole) server address for user device tunnels.
/// Loopback placeholder — every real deployment sets this per-user in the
/// registry (or edits it from the settings page).
pub const DEFAULT_CLOUD_IP: &str = "127.0.0.1";

/// Fleet-default SilverBullet base URL for per-user SB configs. Matches
/// the BFF's own `SB_BASE_URL` default; override per-user as needed.
pub const DEFAULT_SB_BASE_URL: &str = "http://127.0.0.1:3000";

/// Per-user SilverBullet connection config: 域名/账号/密码. Owned by the
/// user (settings page), persisted inside the registry document. Legacy
/// registry entries without an `sb` object deserialize to the defaults
/// (default domain, empty credentials).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserSbConfig {
    /// SB base URL, e.g. `https://sb.example.com`. Must be http(s).
    #[serde(default = "default_sb_base_url")]
    pub base_url: String,
    /// SB account name; empty = not configured yet.
    #[serde(default)]
    pub username: String,
    /// SB password; empty = not configured yet.
    #[serde(default)]
    pub password: String,
}

impl Default for UserSbConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_SB_BASE_URL.to_string(),
            username: String::new(),
            password: String::new(),
        }
    }
}

fn default_sb_base_url() -> String {
    DEFAULT_SB_BASE_URL.to_string()
}

/// Validate a user's SB config: `base_url` must be a non-empty http(s)
/// URL without whitespace (≤200 chars, so it can't smuggle URL syntax);
/// username/password are free-form but length-capped (empty = unset).
pub fn validate_sb_config(sb: &UserSbConfig) -> AppResult<()> {
    let base = sb.base_url.trim();
    let has_scheme = base.starts_with("http://") || base.starts_with("https://");
    if base.is_empty() || base.len() > 200 || !has_scheme || base.chars().any(char::is_whitespace) {
        return Err(AppError::InvalidTarget(format!(
            "invalid sb base_url: {} (must be http(s)://…, no whitespace)",
            sb.base_url
        )));
    }
    if sb.username.len() > 64 {
        return Err(AppError::InvalidTarget("sb username too long (max 64)".into()));
    }
    if sb.password.len() > 128 {
        return Err(AppError::InvalidTarget("sb password too long (max 128)".into()));
    }
    Ok(())
}

fn default_cloud_ip() -> String {
    DEFAULT_CLOUD_IP.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsersFile {
    pub version: u32,
    #[serde(default)]
    pub users: Vec<PortalUser>,
}

fn serialize_users_file(file: &UsersFile) -> AppResult<String> {
    serde_json::to_string_pretty(file)
        .map_err(|e| AppError::Internal(format!("users config serialize: {}", e)))
}

impl Default for UsersFile {
    fn default() -> Self {
        Self {
            version: 1,
            users: Vec::new(),
        }
    }
}

/// Generate a fresh 32-char `[A-Za-z0-9]` login key (62^32 ≈ 2^190 space —
/// collisions are impossible in practice; callers may still re-roll on
/// collision against an existing file).
pub fn generate_key() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..KEY_LEN)
        .map(|_| KEY_ALPHABET[rng.gen_range(0..KEY_ALPHABET.len())] as char)
        .collect()
}

/// Generate a fresh 6-digit numeric user id (`100000..=999999`, no leading
/// zeros). The space is only 9·10^5, so callers MUST dedupe against a
/// freshly-fetched remote registry ([`UsersStore::load`]) — every load
/// hits SB, so no stale window can hide a user another writer appended.
pub fn generate_user_id() -> String {
    use rand::Rng;
    rand::thread_rng().gen_range(100_000..1_000_000).to_string()
}

/// Generate a 6-digit numeric user id guaranteed unique within `uf`.
/// Callers that mint ids MUST pass a registry fetched with
/// [`UsersStore::load`] — the id space is only 9·10^5, so dedupe
/// has to run against the freshest remote state (every load hits SB).
pub fn generate_user_id_unique(uf: &UsersFile) -> String {
    loop {
        let id = generate_user_id();
        if !uf.users.iter().any(|u| u.id == id) {
            return id;
        }
    }
}

/// A well-formed portal user id: exactly 6 ascii digits.
fn is_six_digit_id(s: &str) -> bool {
    s.len() == 6 && s.bytes().all(|b| b.is_ascii_digit())
}

/// Rewrite legacy ids (uuid format, pre-6-digit scheme) to fresh unique
/// 6-digit ids. Returns whether anything changed. Session subjects embed
/// the id, so migrated users must log in again — a one-time cost.
fn migrate_legacy_ids(uf: &mut UsersFile) -> bool {
    let mut changed = false;
    for i in 0..uf.users.len() {
        if !is_six_digit_id(&uf.users[i].id) {
            let new_id = generate_user_id_unique(uf);
            tracing::info!(
                "migrating user {} id {} -> {} (6-digit scheme)",
                uf.users[i].name,
                uf.users[i].id,
                new_id
            );
            uf.users[i].id = new_id;
            changed = true;
        }
    }
    changed
}

/// Strip any explicit `:port` from legacy `public-url` values — the port
/// now lives in the dedicated `port` (TUI) / `oc-port` (opencode) fields,
/// so a legacy `http://host:9464` becomes `http://host` and both composed
/// addresses derive from the fields. Returns whether anything changed.
/// Unparseable values are left untouched for write-time validation.
fn migrate_device_urls(uf: &mut UsersFile) -> bool {
    let mut changed = false;
    for u in &mut uf.users {
        for d in &mut u.devices {
            let Ok(mut parsed) = url::Url::parse(d.public_url.trim()) else {
                continue;
            };
            if parsed.port().is_some() {
                let _ = parsed.set_port(None);
                parsed.set_path("");
                let stripped = parsed.as_str().trim_end_matches('/').to_string();
                tracing::info!(
                    "migrating device {} public-url {} -> {} (port moved to port/oc-port fields)",
                    d.name,
                    d.public_url,
                    stripped
                );
                d.public_url = stripped;
                changed = true;
            }
        }
    }
    changed
}

/// Portal user names follow the device-name whitelist so they stay
/// path/URL-safe wherever we print them. Whitelist:
/// `[A-Za-z0-9_-]{1,64}` (mirrors the device-side whitelist in
/// `routes.rs::validate_pcname`).
pub fn validate_user_name(s: &str) -> AppResult<()> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9_-]{1,64}$").unwrap());
    if re.is_match(s) {
        Ok(())
    } else {
        Err(AppError::InvalidPcname(s.to_string()))
    }
}

/// Validate a user's device list: non-empty; every entry carries a
/// human-readable `desc` (1-64 chars after trim, no control chars);
/// every service name matches `^[a-z0-9-]{1,64}$` (lowercase
/// letters/digits/hyphen ONLY — no underscore, no symbols, no
/// uppercase); every `pctype` is one of [`DEVICE_PCTYPES`] (windows /
/// macos, same enum as the SB path-list platform type); every port is
/// 1-65535; every `public_url` is an http(s) URL (admin-assigned
/// reachable address); no duplicate service names. `device-name` is
/// client-written (not admin-editable) — only length/control-char sanity
/// is enforced.
pub fn validate_user_devices(devices: &[UserDevice]) -> AppResult<()> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^[a-z0-9-]{1,64}$").unwrap());
    if devices.is_empty() {
        return Err(AppError::InvalidTarget("devices must not be empty".into()));
    }
    let mut seen = std::collections::HashSet::new();
    for d in devices {
        let desc = d.desc.trim();
        if desc.is_empty() {
            return Err(AppError::InvalidTarget(format!(
                "bad device description for {}: must not be empty",
                d.name
            )));
        }
        if d.desc.chars().count() > 64 {
            return Err(AppError::InvalidTarget(format!(
                "bad device description for {}: must be at most 64 chars",
                d.name
            )));
        }
        if d.desc.chars().any(char::is_control) {
            return Err(AppError::InvalidTarget(format!(
                "bad device description for {}: control characters not allowed",
                d.name
            )));
        }
        if !DEVICE_PCTYPES.contains(&d.pctype.as_str()) {
            return Err(AppError::InvalidTarget(format!(
                "bad pctype for {}: must be one of {:?}",
                d.name, DEVICE_PCTYPES
            )));
        }
        if d.device_name.chars().count() > 64
            || d.device_name.chars().any(char::is_control)
            || d.device_name.chars().any(|c| "/?#%\\".contains(c))
        {
            return Err(AppError::InvalidTarget(format!(
                "bad device-name for {}: must be at most 64 chars, no control characters, no URL-structure characters (/ ? # % \\)",
                d.name
            )));
        }
        if !re.is_match(&d.name) {
            return Err(AppError::InvalidTarget(format!(
                "bad device name: {} (lowercase letters/digits/hyphen only)",
                d.name
            )));
        }
        if d.port == 0 {
            return Err(AppError::InvalidTarget(format!(
                "bad port for {}: must be 1-65535",
                d.name
            )));
        }
        if d.oc_port == 0 {
            return Err(AppError::InvalidTarget(format!(
                "bad oc-port for {}: must be 1-65535",
                d.name
            )));
        }
        if d.oc_port == d.port {
            return Err(AppError::InvalidTarget(format!(
                "bad oc-port for {}: must differ from the TUI service port {} \
                 (the device TUI rejects equal ports too)",
                d.name, d.port
            )));
        }
        // public_url: a bare tunnel address `http(s)://host` — IP or
        // domain, NO port (ports come from `port` / `oc-port`), NO path.
        // The BFF composes every device URL from it, so bad input is
        // caught here at write time.
        let url = d.public_url.trim();
        if url.is_empty() {
            return Err(AppError::InvalidTarget(format!(
                "bad public-url for {}: must not be empty",
                d.name
            )));
        }
        match url::Url::parse(url) {
            Ok(u)
                if (u.scheme() == "http" || u.scheme() == "https")
                    && u.host_str().is_some()
                    && !u.host_str().unwrap_or("").is_empty()
                    && u.port().is_none()
                    && (u.path() == "/" || u.path().is_empty()) => {}
            Ok(u) => {
                return Err(AppError::InvalidTarget(format!(
                    "bad public-url for {}: {} (must be http(s)://host — \
                     IP or domain, no port, no path)",
                    d.name, u
                )));
            }
            Err(e) => {
                return Err(AppError::InvalidTarget(format!(
                    "bad public-url for {}: {}",
                    d.name, e
                )));
            }
        }
        if !seen.insert(d.name.as_str()) {
            return Err(AppError::InvalidTarget(format!(
                "duplicate device name: {}",
                d.name
            )));
        }
    }
    Ok(())
}

/// Thin store wrapping `SbClient` directly — no in-process TTL. Each call
/// to `load()` hits SB fresh. Rationale (ADR #20): small user base,
/// infrequent clicks, SB can absorb the load; the cache's stale window
/// outweighed its perf benefit.
pub struct UsersStore {
    sb: Arc<SbClient>,
}

impl UsersStore {
    pub fn new(sb: Arc<SbClient>) -> Self {
        Self { sb }
    }

    /// Load `web/opencode/config.md`. A missing document is an empty
    /// registry (first boot — the admin creates the first user). Always
    /// hits SB; no cache.
    pub async fn load(&self) -> AppResult<UsersFile> {
        let body = match self.sb.get_fs(USERS_PATH).await {
            Ok(b) => b,
            Err(AppError::NotFound(_)) => return Ok(UsersFile::default()),
            Err(e) => return Err(e),
        };
        let mut uf: UsersFile = serde_json::from_str(&body)
            .map_err(|e| AppError::Internal(format!("users config parse: {}", e)))?;
        let mut changed = migrate_legacy_ids(&mut uf);
        changed |= migrate_device_urls(&mut uf);
        if changed {
            let body = serialize_users_file(&uf)?;
            self.sb.put_fs(USERS_PATH, &body).await?;
        }
        Ok(uf)
    }

    pub async fn save(&self, file: &UsersFile) -> AppResult<()> {
        let body = serialize_users_file(file)?;
        self.sb.put_fs(USERS_PATH, &body).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_key_is_32_alphanumeric_chars() {
        for _ in 0..1000 {
            let k = generate_key();
            assert_eq!(k.len(), KEY_LEN, "key length: {k}");
            assert!(
                k.chars().all(|c| c.is_ascii_alphanumeric()),
                "key must contain no symbols: {k}"
            );
        }
        // Extremely unlikely to collide twice in a row.
        assert_ne!(generate_key(), generate_key());
    }

    #[test]
    fn generate_user_id_is_6_digits_no_leading_zero() {
        for _ in 0..1000 {
            let id = generate_user_id();
            assert_eq!(id.len(), 6, "id length: {id}");
            assert!(
                id.chars().all(|c| c.is_ascii_digit()),
                "id must be digits only: {id}"
            );
            assert!(
                !id.starts_with('0'),
                "no leading zero (stays 6 digits everywhere): {id}"
            );
        }
    }

    #[test]
    fn migrate_legacy_ids_rewrites_uuids_keeps_6_digit() {
        let user = |id: &str| PortalUser {
            id: id.into(),
            name: format!("user-{id}"),
            key: generate_key(),
            cloud_ip: DEFAULT_CLOUD_IP.into(),
            last_used_at: None,
            devices: Vec::new(),
            sb: UserSbConfig::default(),
            created_at: None,
            updated_at: None,
        };
        let mut uf = UsersFile {
            version: 1,
            users: vec![
                user("11111111-1111-4111-8111-111111111111"),
                user("654321"),
                user("legacy-string"),
            ],
        };
        assert!(migrate_legacy_ids(&mut uf), "two legacy ids rewritten");

        assert_eq!(uf.users[1].id, "654321", "already-6-digit id untouched");
        assert!(
            is_six_digit_id(&uf.users[0].id),
            "uuid becomes 6 digits: {}",
            uf.users[0].id
        );
        assert!(
            is_six_digit_id(&uf.users[2].id),
            "malformed id becomes 6 digits: {}",
            uf.users[2].id
        );

        let mut ids: Vec<&str> = uf.users.iter().map(|u| u.id.as_str()).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "migrated ids stay unique");

        assert!(
            !migrate_legacy_ids(&mut uf),
            "second pass is a no-op (ids stable)"
        );
    }

    #[test]
    fn users_file_round_trips_json() {
        let uf = UsersFile {
            version: 1,
            users: vec![PortalUser {
                id: "11111111-1111-4111-8111-111111111111".into(),
                name: "samuel".into(),
                key: "k1234567890123456789012345678901".into(),
                cloud_ip: DEFAULT_CLOUD_IP.into(),
                last_used_at: None,
                devices: vec![
                    UserDevice {
                        desc: "Samuel 的 MacBook".into(),
                        name: "samuel-mac".into(),
                        port: 9464,
                        oc_port: 18800,
                        device_name: "SAMUEL-MBP".into(),
                        pctype: "macos".into(),
                        bound: true,
                        public_url: "https://oc-mac.example.com".into(),
                    },
                    UserDevice {
                        desc: "Samuel 的 Windows".into(),
                        name: "samuel-win".into(),
                        port: 4040,
                        oc_port: 9464,
                        device_name: String::new(),
                        pctype: "windows".into(),
                        bound: false,
                        public_url: "https://oc-win.example.com".into(),
                    },
                ],
                sb: UserSbConfig {
                    base_url: "https://sb.example.com".into(),
                    username: "samuel".into(),
                    password: "sb-secret".into(),
                },
                created_at: Some("2026-09-13T00:00:00+08:00".into()),
                updated_at: None,
            }],
        };
        let json = serde_json::to_string(&uf).unwrap();
        let parsed: UsersFile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, uf);
    }

    #[test]
    fn missing_sb_config_defaults_to_fleet_domain() {
        // Legacy registry entries (pre-SB-config) carry no `sb` object;
        // they must deserialize with the default domain and empty creds.
        let uf: UsersFile = serde_json::from_str(
            r#"{"version": 1, "users": [{
                "id": "123456",
                "name": "samuel",
                "key": "k1234567890123456789012345678901",
                "devices": []
            }]}"#,
        )
        .unwrap();
        assert_eq!(
            uf.users[0].sb,
            UserSbConfig {
                base_url: DEFAULT_SB_BASE_URL.to_string(),
                username: String::new(),
                password: String::new(),
            }
        );
    }

    #[test]
    fn validate_sb_config_rules() {
        let sb = |base_url: &str, username: &str, password: &str| UserSbConfig {
            base_url: base_url.into(),
            username: username.into(),
            password: password.into(),
        };
        assert!(validate_sb_config(&sb("https://sb.example.com", "", "")).is_ok());
        assert!(validate_sb_config(&sb("http://127.0.0.1:3000", "u", "p")).is_ok());
        assert!(
            validate_sb_config(&sb("", "u", "p")).is_err(),
            "empty base_url rejected"
        );
        assert!(
            validate_sb_config(&sb("ftp://sb.example.com", "u", "p")).is_err(),
            "non-http scheme rejected"
        );
        assert!(
            validate_sb_config(&sb("https://sb.example.com/x y", "u", "p")).is_err(),
            "whitespace rejected"
        );
        assert!(
            validate_sb_config(&sb(&format!("https://{}.com", "a".repeat(200)), "u", "p")).is_err(),
            "overlong base_url rejected"
        );
        assert!(
            validate_sb_config(&sb("https://sb.example.com", &"u".repeat(65), "p")).is_err(),
            "overlong username rejected"
        );
        assert!(
            validate_sb_config(&sb("https://sb.example.com", "u", &"p".repeat(129))).is_err(),
            "overlong password rejected"
        );
    }

    #[test]
    fn validate_user_devices_rules() {
        let ok = |desc: &str, name: &str, port: u16| UserDevice {
            desc: desc.into(),
            name: name.into(),
            port,
            oc_port: 9464,
            device_name: String::new(),
            pctype: "windows".into(),
            bound: false,
            public_url: "https://oc-mac.example.com".into(),
        };
        assert!(validate_user_devices(&[ok("办公本", "a", 1)]).is_ok());
        assert!(validate_user_devices(&[ok("dev 01", "dev-01", 65535)]).is_ok());
        assert!(validate_user_devices(&[]).is_err(), "empty list rejected");
        assert!(
            validate_user_devices(&[ok("a", "a", 1), ok("b", "a", 2)]).is_err(),
            "duplicates rejected"
        );
        assert!(
            validate_user_devices(&[ok("a", "Bad", 1)]).is_err(),
            "uppercase rejected"
        );
        assert!(
            validate_user_devices(&[ok("a", "under_score", 1)]).is_err(),
            "underscore rejected"
        );
        assert!(
            validate_user_devices(&[ok("a", "bad name", 1)]).is_err(),
            "space rejected"
        );
        assert!(
            validate_user_devices(&[ok("a", "a", 0)]).is_err(),
            "port 0 rejected"
        );
        assert!(
            validate_user_devices(&[ok("", "a", 1)]).is_err(),
            "empty desc rejected"
        );
        assert!(
            validate_user_devices(&[ok("   ", "a", 1)]).is_err(),
            "whitespace-only desc rejected"
        );
        assert!(
            validate_user_devices(&[ok(&"长".repeat(65), "a", 1)]).is_err(),
            "overlong desc rejected"
        );
        assert!(
            validate_user_devices(&[ok("a\nb", "a", 1)]).is_err(),
            "control char in desc rejected"
        );
        // pctype must be one of the path-list platform types.
        let mut bad_pct = ok("a", "a", 1);
        bad_pct.pctype = "linux".into();
        assert!(
            validate_user_devices(&[bad_pct]).is_err(),
            "unknown pctype rejected"
        );
        let mut no_pct = ok("a", "a", 1);
        no_pct.pctype = String::new();
        assert!(
            validate_user_devices(&[no_pct]).is_err(),
            "empty pctype rejected"
        );
        let mut mac = ok("a", "a", 1);
        mac.pctype = "macos".into();
        assert!(validate_user_devices(&[mac]).is_ok(), "macos accepted");
        // device-name is client-written; sanity caps still apply.
        let mut long_dn = ok("a", "a", 1);
        long_dn.device_name = "d".repeat(65);
        assert!(
            validate_user_devices(&[long_dn]).is_err(),
            "overlong device-name rejected"
        );
        let mut ctrl_dn = ok("a", "a", 1);
        ctrl_dn.device_name = "a\nb".into();
        assert!(
            validate_user_devices(&[ctrl_dn]).is_err(),
            "control char in device-name rejected"
        );
        // device-name becomes an SB path segment: URL-structure chars
        // would break the path layout, so each one is rejected.
        for bad_char in ['/', '?', '#', '%', '\\'] {
            let mut dn = ok("a", "a", 1);
            dn.device_name = format!("a{bad_char}b");
            assert!(
                validate_user_devices(&[dn]).is_err(),
                "URL-structure char {bad_char:?} in device-name rejected"
            );
        }
        // public-url: empty / non-http / bad scheme / with path → rejected.
        let mut no_url = ok("a", "a", 1);
        no_url.public_url = String::new();
        assert!(
            validate_user_devices(&[no_url]).is_err(),
            "empty public-url rejected"
        );
        let mut ws_url = ok("a", "a", 1);
        ws_url.public_url = "   ".into();
        assert!(
            validate_user_devices(&[ws_url]).is_err(),
            "whitespace-only public-url rejected"
        );
        let mut ftp = ok("a", "a", 1);
        ftp.public_url = "ftp://example.com".into();
        assert!(
            validate_user_devices(&[ftp]).is_err(),
            "non-http(s) scheme rejected"
        );
        let mut with_path = ok("a", "a", 1);
        with_path.public_url = "https://example.com/some/path".into();
        assert!(
            validate_user_devices(&[with_path]).is_err(),
            "public-url with path rejected"
        );
        let mut no_host = ok("a", "a", 1);
        no_host.public_url = "https://".into();
        assert!(
            validate_user_devices(&[no_host]).is_err(),
            "public-url without host rejected"
        );
        // public-url is a bare tunnel address: ports are rejected — they
        // live in the dedicated `port` / `oc-port` fields.
        let mut with_port = ok("a", "a", 1);
        with_port.public_url = "http://10.0.0.1:9464".into();
        assert!(
            validate_user_devices(&[with_port]).is_err(),
            "public-url with port rejected"
        );
        let mut http_ok = ok("a", "a", 1);
        http_ok.public_url = "http://10.0.0.1".into();
        assert!(
            validate_user_devices(&[http_ok]).is_ok(),
            "http://host accepted"
        );
        let mut https_ok = ok("a", "a", 1);
        https_ok.public_url = "https://oc-mac.example.com".into();
        assert!(
            validate_user_devices(&[https_ok]).is_ok(),
            "https://host accepted"
        );
        // oc-port: 0 rejected; equal to the TUI service port rejected
        // (mirrors the device TUI's own submit_settings rule).
        let mut oc_zero = ok("a", "a", 1);
        oc_zero.oc_port = 0;
        assert!(
            validate_user_devices(&[oc_zero]).is_err(),
            "oc-port 0 rejected"
        );
        let mut oc_same = ok("a", "a", 9464);
        oc_same.oc_port = 9464;
        assert!(
            validate_user_devices(&[oc_same]).is_err(),
            "oc-port equal to service port rejected"
        );
    }

    #[test]
    fn missing_fields_default() {
        let uf: UsersFile =
            serde_json::from_str(r#"{"version": 1, "users": []}"#).unwrap();
        assert!(uf.users.is_empty());
    }

    #[test]
    fn user_device_accepts_legacy_registry_shapes() {
        // Pre-设备清单 registry: devices were bare strings and users
        // carried a top-level oc_serve_port. Loading must not fail; the
        // desc/bound schema is likewise optional per entry (legacy
        // entries load with an empty desc and bound=false, any save
        // rewrites the registry in the new format). The legacy `d-name`
        // field deserializes into `desc` via serde alias.
        let uf: UsersFile = serde_json::from_str(
            r#"{"version": 1, "users": [{
                "id": "u1",
                "name": "samuel",
                "key": "k1234567890123456789012345678901",
                "oc_serve_port": 4040,
                "devices": ["home-win", {"name": "dev-b", "port": 9464}, {"name": "dev-c"}, {"d-name": "办公机", "name": "dev-d", "port": 5173, "bound": true}, {"desc": "新格式", "name": "dev-e", "port": 8200, "device-name": "OFFICE-PC", "pctype": "windows"}]
            }]}"#,
        )
        .unwrap();
        let d = |desc: &str, name: &str, port: u16, device_name: &str, pctype: &str, bound: bool| {
            UserDevice {
                desc: desc.into(),
                name: name.into(),
                port,
                oc_port: 9464,
                device_name: device_name.into(),
                pctype: pctype.into(),
                bound,
                public_url: String::new(),
            }
        };
        assert_eq!(
            uf.users[0].devices,
            vec![
                d("", "home-win", LEGACY_DEFAULT_PORT, "", "", false),
                d("", "dev-b", 9464, "", "", false),
                d("", "dev-c", LEGACY_DEFAULT_PORT, "", "", false),
                // legacy `d-name` value lands in `desc` (alias)
                d("办公机", "dev-d", 5173, "", "", true),
                d("新格式", "dev-e", 8200, "OFFICE-PC", "windows", false),
            ]
        );
        // Round-trip: serialization always emits the new field names —
        // `desc` (kebab legacy `d-name` is gone), `device-name`, `pctype`
        // — and `bound`.
        let json = serde_json::to_string(&uf.users[0].devices).unwrap();
        assert!(json.contains(r#""desc":"办公机""#), "serialized: {json}");
        assert!(
            json.contains(r#""device-name":"OFFICE-PC""#),
            "serialized: {json}"
        );
        assert!(json.contains(r#""pctype":"windows""#), "serialized: {json}");
        assert!(json.contains(r#""bound":true"#), "serialized: {json}");
        assert!(!json.contains("d-name"), "legacy field not re-emitted: {json}");
        let parsed: Vec<UserDevice> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, uf.users[0].devices);
    }
}

#[cfg(test)]
mod store_tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn users_store_load_hits_sb_every_call() {
        let sb = MockServer::start().await;
        // Expect TWO GET calls (proves no cache short-circuits the second)
        Mock::given(method("GET"))
            .and(path("/.fs/web/opencode/config.md"))
            .respond_with(ResponseTemplate::new(200).set_body_string(USERS_FILE_BODY))
            .expect(2)
            .mount(&sb)
            .await;
        let store = UsersStore::new(std::sync::Arc::new(
            crate::sb::SbClient::new(sb.uri(), "admin", "pw").unwrap(),
        ));
        // First call
        let _ = store.load().await.unwrap();
        // Second call — must hit SB again (no cache)
        let _ = store.load().await.unwrap();
    }

    const USERS_FILE_BODY: &str = r#"{"version":1,"users":[]}"#;
}
