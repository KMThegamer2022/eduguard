//! Cross-platform agent capabilities behind one interface (GOAL §3 principle 2).
//! Unsupported operations return errors that the console surfaces honestly
//! (feature-gating must be explicit, never silent).

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

pub mod overlay;

#[cfg(target_os = "macos")]
pub use macos::{battery, is_locked, lock_screen, set_muted};
#[cfg(target_os = "windows")]
pub use windows::{battery, is_locked, lock_screen, set_muted};
#[cfg(target_os = "linux")]
pub use linux::{battery, is_locked, lock_screen, set_muted};

use anyhow::{anyhow, Result};
use protocol::AppInfo;

/// Capture the primary monitor as JPEG, scaled to at most `max_width` px.
pub fn capture_jpeg(max_width: u32) -> Result<(Vec<u8>, u32, u32)> {
    let monitors = xcap::Monitor::all().map_err(|e| anyhow!("no monitors: {e}"))?;
    let monitor = monitors.into_iter().next().ok_or_else(|| anyhow!("no monitors found"))?;
    let mut img = monitor
        .capture_image()
        .map_err(|e| anyhow!("capture failed: {e}"))?;
    let (w, h) = (img.width(), img.height());
    if w > max_width {
        let ratio = max_width as f32 / w as f32;
        let new_h = (h as f32 * ratio).max(1.0) as u32;
        img = image::imageops::resize(
            &img,
            max_width,
            new_h,
            image::imageops::FilterType::Triangle,
        );
    }
    let mut out = std::io::Cursor::new(Vec::new());
    // JPEG cannot encode alpha; go through DynamicImage which converts to RGB.
    let dynimg = image::DynamicImage::ImageRgba8(img);
    let (w, h) = (dynimg.width(), dynimg.height());
    dynimg
        .write_to(&mut out, image::ImageFormat::Jpeg)
        .map_err(|e| anyhow!("jpeg encode failed: {e}"))?;
    Ok((out.into_inner(), w, h))
}

/// Foreground window (app name, title). Drives usage logging + keyword scans.
pub fn foreground_window() -> Option<(String, String)> {
    let windows = xcap::Window::all().ok()?;
    for w in &windows {
        if w.is_focused().unwrap_or(false) {
            let app = w.app_name().unwrap_or_default();
            let title = w.title().unwrap_or_default();
            if !app.is_empty() || !title.is_empty() {
                return Some((app, title));
            }
        }
    }
    None
}

/// Best-effort current user (console "active user" column).
pub fn active_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| whoami_fallback())
}

fn whoami_fallback() -> String {
    std::process::Command::new("whoami")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

/// Primary LAN IP via the UDP-connect trick (no packets are sent).
pub fn primary_ip() -> String {
    if let Ok(s) = std::net::UdpSocket::bind("0.0.0.0:0").and_then(|s| {
        s.connect("1.1.1.1:53")?;
        Ok(s.local_addr()?.ip().to_string())
    }) {
        if !s.is_empty() && s != "0.0.0.0" {
            return s;
        }
    }
    String::new()
}

pub fn hostname() -> String {
    sysinfo::System::host_name().unwrap_or_else(|| "unknown".into())
}

pub fn os_label() -> &'static str {
    std::env::consts::OS
}

pub fn open_url(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let c = std::process::Command::new("cmd");
        c
    };
    #[cfg(target_os = "linux")]
    let mut cmd = std::process::Command::new("xdg-open");

    #[cfg(target_os = "windows")]
    {
        cmd.args(["/C", "start", "", url]);
    }
    #[cfg(not(target_os = "windows"))]
    {
        cmd.arg(url);
    }
    let out = cmd.output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "open_url failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

pub fn power(action: PowerAction) -> Result<()> {
    match action {
        PowerAction::Shutdown(d) => shutdown(d),
        PowerAction::Restart(d) => restart(d),
        PowerAction::Logoff => logoff(),
    }
}

pub enum PowerAction {
    Shutdown(u64),
    Restart(u64),
    Logoff,
}

pub fn installed_apps() -> Vec<AppInfo> {
    platform_apps()
}

#[cfg(target_os = "macos")]
fn platform_apps() -> Vec<AppInfo> {
    macos::installed_apps()
}

#[cfg(target_os = "windows")]
fn platform_apps() -> Vec<AppInfo> {
    windows::installed_apps()
}

#[cfg(target_os = "linux")]
fn platform_apps() -> Vec<AppInfo> {
    linux::installed_apps()
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn platform_apps() -> Vec<AppInfo> {
    Vec::new()
}

#[cfg(target_os = "macos")]
fn shutdown(delay: u64) -> Result<()> {
    macos::power_cmd("shut down", delay)
}
#[cfg(target_os = "macos")]
fn restart(delay: u64) -> Result<()> {
    macos::power_cmd("restart", delay)
}
#[cfg(target_os = "macos")]
fn logoff() -> Result<()> {
    macos::power_cmd("log out", 0)
}

#[cfg(target_os = "windows")]
fn shutdown(delay: u64) -> Result<()> {
    windows::power_cmd("/s", delay)
}
#[cfg(target_os = "windows")]
fn restart(delay: u64) -> Result<()> {
    windows::power_cmd("/r", delay)
}
#[cfg(target_os = "windows")]
fn logoff() -> Result<()> {
    windows::power_cmd("/l", 0)
}

#[cfg(target_os = "linux")]
fn shutdown(delay: u64) -> Result<()> {
    linux::power_cmd("poweroff", delay)
}
#[cfg(target_os = "linux")]
fn restart(delay: u64) -> Result<()> {
    linux::power_cmd("reboot", delay)
}
#[cfg(target_os = "linux")]
fn logoff() -> Result<()> {
    linux::logoff()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_ip_parses() {
        let ip = primary_ip();
        assert!(ip.is_empty() || ip.parse::<std::net::IpAddr>().is_ok(), "{ip}");
    }

    #[test]
    fn foreground_does_not_panic() {
        let _ = foreground_window();
    }
}
