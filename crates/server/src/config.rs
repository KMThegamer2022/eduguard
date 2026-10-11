//! Server configuration. File is TOML; missing values get sane defaults and a
//! first run generates the admin password + enrollment token.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// e.g. "0.0.0.0:8443"
    pub listen: String,
    /// Backing store directory (SQLite + screenshots).
    pub data_dir: PathBuf,
    /// Console admin username.
    pub admin_username: String,
    /// Plaintext only until first run replaces it with a generated secret.
    #[serde(default)]
    pub admin_password: String,
    /// School name shown on student-facing overlays.
    pub school_name: String,
    /// Seconds before a console session expires.
    pub session_ttl_secs: u64,
    /// Screenshot retention (days).
    pub screenshot_retention_days: u64,
    /// Event/audit retention (days).
    pub event_retention_days: u64,
    /// Consider a device offline after this many seconds without heartbeat.
    pub offline_after_secs: u64,
    /// TLS certificate (PEM). When set with `tls_key`, serve HTTPS/WSS.
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
}

fn default_listen() -> String {
    "0.0.0.0:8443".to_string()
}

fn default_data_dir() -> PathBuf {
    if cfg!(target_os = "windows") {
        PathBuf::from(r"C:\ProgramData\EduGuard\server")
    } else if cfg!(target_os = "macos") {
        PathBuf::from("/Library/Application Support/EduGuard/server")
    } else {
        PathBuf::from("/var/lib/eduguard")
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            listen: default_listen(),
            data_dir: default_data_dir(),
            admin_username: "admin".to_string(),
            admin_password: String::new(),
            school_name: "EduGuard School".to_string(),
            session_ttl_secs: 12 * 60 * 60,
            screenshot_retention_days: 30,
            event_retention_days: 365,
            offline_after_secs: 20,
            tls_cert: None,
            tls_key: None,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        if path.exists() {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            let cfg: Config = toml::from_str(&text).context("parsing server config")?;
            Ok(cfg)
        } else {
            let cfg = Config::default();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let text = toml::to_string_pretty(&cfg)?;
            std::fs::write(path, text)
                .with_context(|| format!("writing default config to {}", path.display()))?;
            Ok(cfg)
        }
    }

    pub fn default_path() -> PathBuf {
        if cfg!(target_os = "windows") {
            PathBuf::from(r"C:\ProgramData\EduGuard\server\server.toml")
        } else {
            PathBuf::from("/etc/eduguard/server.toml")
        }
    }

    pub fn tls_enabled(&self) -> bool {
        self.tls_cert.is_some() && self.tls_key.is_some()
    }
}

/// Generate a URL-safe random secret.
pub fn random_token(bytes: usize) -> String {
    use rand::RngCore;
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}
