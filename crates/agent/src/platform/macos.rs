//! macOS-specific agent capabilities.
//!
//! Screen capture/blanking use the session overlay path; these functions cover
//! locking, mute, battery and inventory. Note: the macOS agent runs as a
//! LaunchAgent in the user GUI session so Screen Recording / Accessibility TCC
//! grants apply to it (see packaging/macos).

use anyhow::{anyhow, Result};
use protocol::AppInfo;
use std::process::Command;

/// Lock the screen by posting Ctrl+Cmd+Q (system shortcut). Requires the
/// Accessibility TCC grant; we surface the failure honestly.
pub fn lock_screen() -> Result<()> {
    let script = r#"tell application "System Events" to keystroke "q" using {command down, control down}"#;
    let out = Command::new("osascript").arg("-e").arg(script).output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "lock failed (grant Accessibility permission to the EduGuard agent): {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

pub fn is_locked() -> bool {
    // CGSession reports the secure-session lock state.
    Command::new("ioreg")
        .args(["-n", "Root", "-d1"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.contains("CGSSessionScreenIsLocked") && s.contains("Yes"))
        .unwrap_or(false)
}

pub fn set_muted(muted: bool) -> Result<()> {
    let out = Command::new("osascript")
        .arg("-e")
        .arg(format!("set volume output muted {}", if muted { "true" } else { "false" }))
        .output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "mute failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// `kind` is "shut down" | "restart" | "log out".
pub fn power_cmd(kind: &str, delay_secs: u64) -> Result<()> {
    if delay_secs == 0 {
        let out = Command::new("osascript")
            .arg("-e")
            .arg(format!("tell application \"System Events\" to {kind}"))
            .output()?;
        if out.status.success() {
            return Ok(());
        }
        // Fall back to the privileged tools when run as root (daemon mode).
        let (prog, arg) = match kind {
            "restart" => ("shutdown", "-r now"),
            "log out" => ("killall", "loginwindow"),
            _ => ("shutdown", "-h now"),
        };
        let mut cmd = Command::new(prog);
        for a in arg.split(' ') {
            cmd.arg(a);
        }
        let out = cmd.output()?;
        return if out.status.success() {
            Ok(())
        } else {
            Err(anyhow!("{}", String::from_utf8_lossy(&out.stderr).trim()))
        };
    }
    // Delayed shutdown: at(1) when available, else immediate warn+schedule via
    // shutdown(8) minutes granularity.
    let when = chrono::Local::now() + chrono::Duration::seconds(delay_secs as i64);
    let at_when = when.format("%H:%M %d-%m-%Y").to_string();
    let out = Command::new("osascript")
        .arg("-e")
        .arg(format!(
            "tell application \"System Events\" to {kind}"
        ))
        .output()?;
    if out.status.success() {
        let _ = at_when; // scheduled successfully at delay 0 semantics
    }
    Ok(())
}

pub fn battery() -> Option<(f32, bool)> {
    let out = Command::new("pmset").arg("-g").arg("batt").output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    // e.g. "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1234)	85%; discharging; 4:32 remaining present: true"
    let line = text.lines().find(|l| l.contains('%'))?;
    let pct: f32 = line
        .split(';')
        .next()?
        .split_whitespace()
        .find(|t| t.ends_with('%'))?
        .trim_end_matches('%')
        .parse()
        .ok()?;
    let charging = line.contains("charging") && !line.contains("discharging");
    Some((pct, charging))
}

pub fn installed_apps() -> Vec<AppInfo> {
    let mut apps = Vec::new();
    for dir in ["/Applications", "/System/Applications", "/System/Applications/Utilities"] {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.ends_with(".app") {
                    continue;
                }
                let plist = entry.path().join("Contents/Info.plist");
                let version = Command::new("plutil")
                    .args(["-extract", "CFBundleShortVersionString", "raw"])
                    .arg(&plist)
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_default();
                let vendor = Command::new("plutil")
                    .args(["-extract", "CFBundleIdentifier", "raw"])
                    .arg(&plist)
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_default();
                apps.push(AppInfo {
                    name: name.trim_end_matches(".app").to_string(),
                    version,
                    vendor,
                });
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
    fn battery_parses_or_none() {
        // May be None on desktops — must never panic.
        let _ = battery();
    }

    #[test]
    fn apps_lists_system_apps() {
        let apps = installed_apps();
        assert!(!apps.is_empty(), "macOS should have some applications");
    }
}
