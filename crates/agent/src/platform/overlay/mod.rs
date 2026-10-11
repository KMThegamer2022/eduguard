//! Student-facing overlay windows: monitoring banner (transparency guardrail),
//! full-screen blanking, and teacher messages. Each OS implements the window
//! plumbing; this module owns the control channel.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub enum Update {
    /// Fullscreen black blanking.
    Blank(bool),
    /// Transparency banner text; None disables.
    Banner(Option<String>),
    /// Teacher message with TTL in seconds.
    Message { text: String, ttl_secs: u64 },
    ClearMessage,
    /// Shut the overlay thread down (agent exit).
    Quit,
}

static TX: OnceLock<Sender<Update>> = OnceLock::new();

pub fn sender() -> &'static Sender<Update> {
    TX.get_or_init(|| {
        let (tx, rx) = channel();
        std::thread::Builder::new()
            .name("eduguard-overlay".into())
            .spawn(move || run_loop(rx))
            .expect("spawn overlay thread");
        tx
    })
}

/// macOS only: the process main thread must host AppKit, so main() creates the
/// channel itself and installs it before any update is sent.
#[cfg(target_os = "macos")]
pub fn install(tx: Sender<Update>) {
    let _ = TX.set(tx);
}

/// Run the overlay loop on the *current* thread (macOS main-thread path).
#[cfg(target_os = "macos")]
pub fn run_current_thread(rx: Receiver<Update>) {
    run_loop(rx);
}

#[allow(dead_code)] // used on non-macOS targets
pub fn ensure_started() {
    let _ = sender();
}

pub fn send(update: Update) {
    let _ = sender().send(update);
}

/// Current on-screen state, shared with the heartbeat and tray icon.
pub mod state {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    pub static BLANKED: AtomicBool = AtomicBool::new(false);
    pub fn blanked() -> bool {
        BLANKED.load(Ordering::Relaxed)
    }

    /// Base URL of the management console, for the tray/menu-bar icon's
    /// "Open Console" action. Set once the agent connects; empty until then.
    static CONSOLE_URL: Mutex<String> = Mutex::new(String::new());

    pub fn set_console_url(url: &str) {
        if let Ok(mut g) = CONSOLE_URL.lock() {
            *g = url.trim_end_matches('/').to_string();
        }
    }

    pub fn console_url() -> String {
        CONSOLE_URL.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

fn run_loop(rx: Receiver<Update>) {
    #[cfg(target_os = "macos")]
    macos::run(rx);
    #[cfg(target_os = "windows")]
    windows::run(rx);
    #[cfg(target_os = "linux")]
    linux::run(rx);
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = rx;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn update_debug_shapes() {
        let u = super::Update::Message { text: "hi".into(), ttl_secs: 5 };
        assert!(format!("{u:?}").contains("hi"));
    }
}
