//! Agent configuration + persisted device identity.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentConfig {
    /// Server base URL, e.g. "http://192.168.1.10:8443" or "https://school.example:8443"
    #[serde(default)]
    pub server: String,
    /// Optional friendly device name shown in the console.
    #[serde(default)]
    pub name: String,
    /// Optional enrollment token baked in by the installer.
    #[serde(default)]
    pub enrollment_token: String,
    /// Log filter, e.g. "info" or "eduguard_agent=debug".
    #[serde(default)]
    pub log: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub device_id: String,
    pub device_token: String,
    pub server: String,
}

pub struct Paths {
    pub config: PathBuf,
    pub identity: PathBuf,
    pub downloads: PathBuf,
}

/// Default locations:
///   Windows: C:\ProgramData\EduGuard\
///   macOS:   /Library/Application Support/EduGuard/
///   Linux:   /etc/eduguard/ + /var/lib/eduguard/
pub fn default_paths() -> Paths {
    if cfg!(target_os = "windows") {
        let base = PathBuf::from(r"C:\ProgramData\EduGuard");
        Paths {
            config: base.join("agent.toml"),
            identity: base.join("identity.json"),
            downloads: PathBuf::from(r"C:\Users\Public\Downloads"),
        }
    } else if cfg!(target_os = "macos") {
        let base = PathBuf::from("/Library/Application Support/EduGuard");
        Paths {
            config: base.join("agent.toml"),
            identity: base.join("identity.json"),
            downloads: home_dir().join("Downloads"),
        }
    } else {
        Paths {
            config: PathBuf::from("/etc/eduguard/agent.toml"),
            identity: PathBuf::from("/var/lib/eduguard/agent-identity.json"),
            downloads: home_dir().join("Downloads"),
        }
    }
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

impl AgentConfig {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        if path.exists() {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            Ok(toml::from_str(&text).context("parsing agent.toml")?)
        } else {
            Ok(AgentConfig::default())
        }
    }

    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, toml::to_string_pretty(self)?)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}

impl Identity {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        if !path.exists() {
            bail!(
                "not enrolled: {} is missing.\nEnroll with:\n  eduguard-agent enroll --server <url> --token <enrollment-token>",
                path.display()
            );
        }
        let text = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&text).context("parsing identity.json")?)
    }

    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip() {
        let dir = std::env::temp_dir().join(format!("eg-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("agent.toml");
        let cfg = AgentConfig {
            server: "http://localhost:8443".into(),
            name: "Lab-1".into(),
            enrollment_token: "eg-x".into(),
            log: "info".into(),
        };
        cfg.save(&p).unwrap();
        let back = AgentConfig::load(&p).unwrap();
        assert_eq!(back.server, cfg.server);
        assert_eq!(back.enrollment_token, "eg-x");
        std::fs::remove_dir_all(&dir).ok();
    }
}
