//! EduGuard wire protocol v1.
//!
//! Everything that crosses the network between a device agent and the server is
//! defined here, so both binaries stay in lock-step. Messages are JSON envelopes
//! (`ClientMessage` / `ServerMessage`) carried over a WebSocket. The protocol is
//! versioned: agents declare `PROTOCOL_VERSION` at hello and the server rejects
//! incompatible majors.

use serde::{Deserialize, Serialize};

/// Bumped on breaking changes. Agents with a different major are rejected.
pub const PROTOCOL_VERSION: u32 = 1;
pub const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

// ---------------------------------------------------------------------------
// Messages: agent -> server
// ---------------------------------------------------------------------------

/// First frame an agent sends on the device WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    pub device_id: String,
    pub token: String,
    pub agent_version: String,
}

/// Everything the server needs to mint an identity for a device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentRequest {
    /// Shared enrollment token distributed to the lab/school.
    pub enrollment_token: String,
    /// Optional human-friendly name chosen at install time.
    pub device_name: Option<String>,
    pub hostname: String,
    pub os: String,
    pub os_version: String,
    pub arch: String,
    pub agent_version: String,
    pub enrolled_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentResponse {
    pub device_id: String,
    pub device_token: String,
    pub server_version: String,
}

/// Periodic liveness + machine state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heartbeat {
    pub ts: i64,
    pub cpu_pct: f32,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub battery_pct: Option<f32>,
    pub charging: Option<bool>,
    pub active_user: String,
    pub screen_locked: bool,
    pub primary_ip: String,
    pub uptime_secs: u64,
}

/// Software/hardware inventory snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inventory {
    pub ts: i64,
    pub os: String,
    pub os_version: String,
    pub hostname: String,
    pub arch: String,
    pub cpu: String,
    pub cpu_cores: u32,
    pub mem_total_mb: u64,
    pub disks: Vec<DiskInfo>,
    pub installed_apps: Vec<AppInfo>,
    pub peripherals: Vec<String>,
    pub agent_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    pub name: String,
    pub total_mb: u64,
    pub available_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub vendor: String,
}

/// One sampled foreground window (usage logging, C1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivitySample {
    pub ts: i64,
    pub app: String,
    pub title: String,
    /// Best-effort URL when the foreground window is a browser tab.
    pub url: Option<String>,
}

/// Safeguarding alert raised by the agent-side keyword pipeline (A1/A2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordAlert {
    pub ts: i64,
    pub category: String,
    pub keyword: String,
    pub context: String,
    pub source: AlertSource,
    pub app: String,
    pub severity: Severity,
    /// Path (server-side) or bytes are attached out-of-band; this is metadata.
    pub evidence_ref: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertSource {
    /// Matched against a browser URL or page title.
    Url,
    /// Matched against a window/app title.
    WindowTitle,
    /// Matched against text typed into the console channel (opt-in mode).
    TypedText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

/// Student pressed "raise hand" / asked for help (B9).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaiseHand {
    pub ts: i64,
    pub note: Option<String>,
}

/// Chat message from student -> staff (B5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub ts: i64,
    pub body: String,
}

/// Agent -> server envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Hello(Hello),
    Heartbeat(Heartbeat),
    Inventory(Box<Inventory>),
    /// Struct variant (not `Activity(Vec<..>)`): internally-tagged enums can
    /// only wrap maps, so a tuple variant over a sequence fails to serialize.
    Activity {
        samples: Vec<ActivitySample>,
    },
    Alert(KeywordAlert),
    Screenshot {
        device_id: String,
        ts: i64,
        /// JPEG bytes, base64.
        jpeg_b64: String,
        width: u32,
        height: u32,
    },
    RaiseHand(RaiseHand),
    Chat(ChatMessage),
    Ack {
        command_id: String,
        ok: bool,
        detail: Option<String>,
    },
    /// Agent-side log line surfaced in the console device detail view.
    Log {
        ts: i64,
        level: String,
        message: String,
    },
}

// ---------------------------------------------------------------------------
// Messages: server -> agent
// ---------------------------------------------------------------------------

/// A policy pushed by the server; the agent's single source of truth for
/// enforced state (E4). Applied on connect and on every change.
///
/// Default posture is *discreet*: no on-screen banner, no capture notices.
/// Transparency affordances exist but are opt-in per school policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    /// Fullscreen privacy/monitoring banner on student devices (guardrail #1).
    /// Off by default — enable per-school in the console.
    #[serde(default)]
    pub monitor_banner: bool,
    /// School name rendered in the banner / message overlay.
    #[serde(default = "d_school")]
    pub school_name: String,
    /// Teacher locked the class (state).
    #[serde(default)]
    pub locked: bool,
    /// Teacher blanked the class (state).
    #[serde(default)]
    pub blanked: bool,
    /// Class muted (state).
    #[serde(default)]
    pub muted: bool,
    /// How often the agent samples the foreground window. 0 disables.
    #[serde(default = "d_activity")]
    pub activity_interval_secs: u64,
    /// Enabled safeguarding categories (empty = all).
    #[serde(default)]
    pub keyword_categories: Vec<String>,
    /// Keyword rows are shipped by the server so libraries stay centrally
    /// managed (A2).
    #[serde(default)]
    pub keywords: Vec<KeywordRule>,
    /// Show the "capture evidence" indicator when a screenshot is taken.
    /// Off by default: schools that want visible capture notices can opt in
    /// from the console (Settings → Default policy).
    #[serde(default)]
    pub capture_indicator: bool,
}

fn d_school() -> String {
    "EduGuard School".to_string()
}
fn d_activity() -> u64 {
    30
}

impl Default for Policy {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordRule {
    pub keyword: String,
    pub category: String,
    pub severity: Severity,
    #[serde(default)]
    pub note: String,
}

/// Discrete actions. Stateful controls also live in [`Policy`] so offline
/// devices converge; these carry the one-shot commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    /// Take a screenshot now.
    Screenshot { max_width: u32 },
    /// Show a message overlay on the student screen (B5).
    Message { text: String, ttl_secs: u64 },
    /// Clear any visible message overlay.
    ClearMessage,
    /// Open a URL in the default browser (B2 assist / C8).
    OpenUrl { url: String },
    /// Power actions (C8).
    Shutdown { delay_secs: u64 },
    Restart { delay_secs: u64 },
    Logoff,
    /// Push a file to the device (B6): path server-side, delivered as bytes.
    SendFile { name: String, jpeg_b64_data: String },
    /// Force a fresh inventory scan.
    RescanInventory,
    /// Agent asks to show the raise-hand state again (no-op helper).
    Nop,
}

/// Server -> agent envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    HelloOk {
        server_version: String,
        policy: Policy,
        queued: Vec<Command>,
    },
    HelloDenied {
        reason: String,
    },
    Command {
        command_id: String,
        command: Command,
    },
    Policy(Policy),
    Chat(ChatMessage),
    /// Someone is watching live thumbnails right now (drives FPS adaptivity).
    Viewing {
        active: bool,
    },
}

// ---------------------------------------------------------------------------
// REST shapes shared with the console
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub hostname: String,
    pub os: String,
    pub os_version: String,
    pub arch: String,
    pub agent_version: String,
    pub enrolled_at: i64,
    pub last_seen: Option<i64>,
    pub online: bool,
    pub active_user: String,
    pub primary_ip: String,
    pub cpu_pct: f32,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub battery_pct: Option<f32>,
    pub screen_locked: bool,
    pub thumbnail_ts: Option<i64>,
    pub policy: Policy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRow {
    pub id: i64,
    pub ts: i64,
    pub device_id: String,
    pub device_name: String,
    pub kind: String,
    pub severity: String,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRow {
    pub id: i64,
    pub ts: i64,
    pub actor: String,
    pub action: String,
    pub target: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    pub token: String,
    pub username: String,
    pub role: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_default_is_sane() {
        let p = Policy::default();
        // Discreet by default: no banner, no capture notices.
        assert!(!p.monitor_banner);
        assert!(!p.capture_indicator);
        assert_eq!(p.activity_interval_secs, 30);
        assert!(!p.locked);
    }

    #[test]
    fn command_envelope_roundtrip() {
        let msg = ServerMessage::Command {
            command_id: "c1".into(),
            command: Command::Screenshot { max_width: 320 },
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains("\"type\":\"command\""));
        let back: ServerMessage = serde_json::from_str(&s).unwrap();
        match back {
            ServerMessage::Command { command, .. } => match command {
                Command::Screenshot { max_width } => assert_eq!(max_width, 320),
                _ => panic!("wrong command"),
            },
            _ => panic!("wrong message"),
        }
    }

    #[test]
    fn client_messages_tagged() {
        let m = ClientMessage::Ack {
            command_id: "x".into(),
            ok: true,
            detail: None,
        };
        let s = serde_json::to_string(&m).unwrap();
        assert_eq!(s, "{\"type\":\"ack\",\"command_id\":\"x\",\"ok\":true,\"detail\":null}");
    }

    /// Regression: internally-tagged newtype variants can only wrap maps.
    /// `Activity(Vec<..>)` made serialization fail at runtime, and the agent's
    /// writer dropped the frame silently — activity events never reached the
    /// server. Struct variants are the supported shape.
    #[test]
    fn activity_roundtrips() {
        let m = ClientMessage::Activity {
            samples: vec![ActivitySample {
                ts: 1,
                app: "TextEdit".into(),
                title: "draft.txt".into(),
                url: None,
            }],
        };
        let s = serde_json::to_string(&m).expect("activity must serialize");
        assert!(s.contains("\"type\":\"activity\""), "{s}");
        let back: ClientMessage = serde_json::from_str(&s).expect("activity must parse");
        match back {
            ClientMessage::Activity { samples } => {
                assert_eq!(samples.len(), 1);
                assert_eq!(samples[0].app, "TextEdit");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }
}
