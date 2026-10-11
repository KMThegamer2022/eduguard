//! Linux-specific agent capabilities (X11 + systemd/logind world).

use anyhow::{anyhow, Result};
use protocol::AppInfo;
use std::process::Command;

pub fn lock_screen() -> Result<()> {
    // xdg-screensaver covers X11; loginctl covers Wayland sessions.
    for (prog, args) in [
        ("xdg-screensaver", vec!["lock"]),
        ("loginctl", vec!["lock-session"]),
    ] {
        match Command::new(prog).args(&args).status() {
            Ok(st) if st.success() => return Ok(()),
            Ok(_) => continue,
            Err(_) => continue,
        }
    }
    Err(anyhow!(
        "no locker available (install xdg-screensaver or systemd-logind)"
    ))
}

pub fn is_locked() -> bool {
    Command::new("xdg-screensaver")
        .args(["status"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.contains("locked"))
        .unwrap_or(false)
}

pub fn set_muted(muted: bool) -> Result<()> {
    let state = if muted { "1" } else { "0" };
    for (prog, args) in [
        ("pactl", vec!["set-sink-mute", "@DEFAULT_SINK@", state]),
        ("amixer", vec!["set", "Master", if muted { "mute" } else { "unmute" }]),
    ] {
        match Command::new(prog).args(&args).status() {
            Ok(st) if st.success() => return Ok(()),
            _ => continue,
        }
    }
    Err(anyhow!("no audio control found (pactl/amixer)"))
}

pub fn power_cmd(kind: &str, delay_secs: u64) -> Result<()> {
    // systemd path: schedule (or immediately start) poweroff.target/reboot.target.
    let mut args: Vec<String> = vec!["--no-block".into()];
    if delay_secs > 0 {
        args.push(format!("--timer={delay_secs}s"));
    }
    args.push(format!("{kind}.target"));
    let out = Command::new("systemctl").args(&args).output()?;
    if out.status.success() {
        return Ok(());
    }
    // Fallbacks for non-systemd hosts.
    let prog = if kind == "poweroff" { "poweroff" } else { "reboot" };
    let out = Command::new(prog).output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "power action failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

pub fn logoff() -> Result<()> {
    // Best-effort: logind terminate the user's own session.
    if let Ok(user) = std::env::var("USER") {
        let out = Command::new("loginctl")
            .args(["terminate-user", &user])
            .output()?;
        if out.status.success() {
            return Ok(());
        }
    }
    Err(anyhow!("logoff requires systemd-logind"))
}

pub fn battery() -> Option<(f32, bool)> {
    let base = std::path::Path::new("/sys/class/power_supply");
    for entry in std::fs::read_dir(base).ok()? {
        let entry = entry.ok()?;
        if !entry.file_name().to_string_lossy().starts_with("BAT") {
            continue;
        }
        let capacity = std::fs::read_to_string(entry.path().join("capacity")).ok()?;
        let pct: f32 = capacity.trim().parse().ok()?;
        let status = std::fs::read_to_string(entry.path().join("status")).unwrap_or_default();
        return Some((pct, status.contains("Charging")));
    }
    None
}

pub fn installed_apps() -> Vec<AppInfo> {
    let mut apps = Vec::new();
    // Debian/Ubuntu
    if let Ok(out) = Command::new("dpkg-query")
        .args(["-W", "-f=${Package}\t${Version}\t${Maintainer}\n"])
        .output()
    {
        if out.status.success() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                let mut it = line.split('\t');
                let (Some(name), Some(version), vendor) = (it.next(), it.next(), it.next())
                else {
                    continue;
                };
                apps.push(AppInfo {
                    name: name.to_string(),
                    version: version.to_string(),
                    vendor: vendor.unwrap_or_default().to_string(),
                });
            }
        }
    }
    // RHEL/Fedora
    if apps.is_empty() {
        if let Ok(out) = Command::new("rpm").args(["-qa", "--queryformat", "%{NAME}\t%{VERSION}-%{RELEASE}\t%{VENDOR}\n"]).output() {
            if out.status.success() {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    let mut it = line.split('\t');
                    let (Some(name), Some(version), vendor) = (it.next(), it.next(), it.next())
                    else {
                        continue;
                    };
                    apps.push(AppInfo {
                        name: name.to_string(),
                        version: version.to_string(),
                        vendor: vendor.unwrap_or_default().to_string(),
                    });
                }
            }
        }
    }
    // Fallback: .desktop entries.
    if apps.is_empty() {
        for dir in ["/usr/share/applications", "/var/lib/snapd/desktop/applications"] {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    if entry.path().extension().map(|e| e == "desktop").unwrap_or(false) {
                        if let Ok(text) = std::fs::read_to_string(entry.path()) {
                            let name = text
                                .lines()
                                .find(|l| l.starts_with("Name="))
                                .map(|l| l.trim_start_matches("Name=").to_string())
                                .unwrap_or_default();
                            if !name.is_empty() {
                                apps.push(AppInfo {
                                    name,
                                    version: String::new(),
                                    vendor: String::new(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps.dedup_by(|a, b| a.name == b.name);
    apps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_never_panics() {
        let _ = battery();
    }
}
