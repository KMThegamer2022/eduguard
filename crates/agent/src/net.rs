//! Agent connection core: WebSocket session, heartbeats, command execution,
//! activity sampling and keyword alerting.

use crate::config::{home_dir, Identity};
use crate::platform::{self, overlay, PowerAction};
use crate::scan::{url_from_title, Scanner};
use anyhow::{anyhow, Context, Result};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use protocol::{
    ActivitySample, ChatMessage, ClientMessage, Command, Heartbeat, Hello, Policy, ServerMessage,
};
use std::sync::{Arc, Mutex};
use sysinfo::System;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

/// Everything the running agent needs; cheap to clone into command tasks.
pub struct AgentCtx {
    pub device_id: String,
    pub downloads: std::path::PathBuf,
    pub policy: Arc<Mutex<Policy>>,
    pub out: mpsc::Sender<ClientMessage>,
}

impl AgentCtx {
    async fn log(&self, level: &str, message: &str) {
        tracing::warn!(target: "eduguard_agent", "[{level}] {message}");
        let _ = self
            .out
            .send(ClientMessage::Log {
                ts: chrono::Utc::now().timestamp(),
                level: level.to_string(),
                message: message.to_string(),
            })
            .await;
    }
}

pub struct Agent {
    pub identity: Identity,
    pub downloads: std::path::PathBuf,
    pub system: Mutex<System>,
}

impl Agent {
    pub fn new(identity: Identity, downloads: std::path::PathBuf) -> Self {
        Agent {
            identity,
            downloads,
            system: Mutex::new(System::new()),
        }
    }

    /// Run forever with reconnect/backoff (offline-tolerant: last-known policy
    /// keeps being enforced locally while disconnected).
    pub async fn run(&self) {
        let mut backoff = 2u64;
        loop {
            match self.session().await {
                Ok(()) => backoff = 2,
                Err(e) => tracing::warn!(error = %format!("{e:#}"), "session ended; reconnect in {backoff}s"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
            backoff = (backoff * 2).min(60);
        }
    }

    fn ws_url(&self) -> String {
        to_ws_url(&self.identity.server, "/ws/device")
    }

    async fn session(&self) -> Result<()> {
        let url = self.ws_url();
        let (ws, _) = tokio_tungstenite::connect_async(&url)
            .await
            .with_context(|| format!("connecting to {url}"))?;
        tracing::info!(server = %self.identity.server, "connected");
        // Let the tray icon ("Open Console") know where the console lives.
        overlay::state::set_console_url(&self.identity.server);
        let (mut ws_tx, mut ws_rx) = ws.split();

        let hello = ClientMessage::Hello(Hello {
            protocol: protocol::PROTOCOL_VERSION,
            device_id: self.identity.device_id.clone(),
            token: self.identity.device_token.clone(),
            agent_version: protocol::AGENT_VERSION.to_string(),
        });
        ws_tx
            .send(Message::Text(serde_json::to_string(&hello)?.into()))
            .await?;

        let first = ws_rx
            .next()
            .await
            .ok_or_else(|| anyhow!("server closed before hello"))??;
        let text = first.to_text()?.to_string();
        let policy = Arc::new(Mutex::new(Policy::default()));
        match serde_json::from_str::<ServerMessage>(&text)? {
            ServerMessage::HelloOk {
                policy: pol,
                queued,
                server_version,
            } => {
                tracing::info!(%server_version, "hello accepted");
                apply_policy(&pol, &policy);
                let (out_tx, mut out_rx) = mpsc::channel::<ClientMessage>(256);
                let ctx = AgentCtx {
                    device_id: self.identity.device_id.clone(),
                    downloads: self.downloads.clone(),
                    policy: policy.clone(),
                    out: out_tx.clone(),
                };
                for cmd in queued {
                    handle_command(&ctx, cmd).await;
                }

                // Fan-out: application messages -> websocket.
                let writer = tokio::spawn(async move {
                    while let Some(msg) = out_rx.recv().await {
                        let text = match serde_json::to_string(&msg) {
                            Ok(t) => t,
                            Err(e) => {
                                // Never drop silently again — this hid the
                                // activity-event bug for the whole smoke test.
                                tracing::warn!(error = %e, "serialize client message failed");
                                continue;
                            }
                        };
                        if ws_tx.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                });

                let result = self
                    .pump(&ctx, &mut ws_rx, &policy, &out_tx)
                    .await;
                writer.abort();
                result
            }
            ServerMessage::HelloDenied { reason } => {
                Err(anyhow!("server denied hello: {reason}"))
            }
            other => Err(anyhow!("unexpected hello response: {other:?}")),
        }
    }

    /// Main message pump while connected.
    async fn pump(
        &self,
        ctx: &AgentCtx,
        ws_rx: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
        policy: &Arc<Mutex<Policy>>,
        out: &mpsc::Sender<ClientMessage>,
    ) -> Result<()> {
        // Prime metrics + initial inventory.
        {
            let mut sys = self.system.lock().unwrap();
            sys.refresh_cpu_all();
            sys.refresh_memory();
        }
        if let Some(hb) = self.heartbeat() {
            out.send(ClientMessage::Heartbeat(hb)).await.ok();
        }
        {
            let inv = {
                let mut sys = self.system.lock().unwrap();
                crate::inventory::collect(&mut sys)
            }; // drop the guard before awaiting (System is !Send on Windows)
            out.send(ClientMessage::Inventory(Box::new(inv))).await.ok();
        }

        let mut heart = tokio::time::interval(std::time::Duration::from_secs(5));
        heart.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut inventory = tokio::time::interval(std::time::Duration::from_secs(30 * 60));
        inventory.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut beat: u64 = 0;

        loop {
            tokio::select! {
                _ = heart.tick() => {
                    beat += 1;
                    if let Some(hb) = self.heartbeat() {
                        if out.send(ClientMessage::Heartbeat(hb)).await.is_err() {
                            break;
                        }
                    }
                    // Activity sampling on the policy cadence (heartbeat-driven
                    // so policy changes apply without rebuilding timers).
                    let interval = { policy.lock().unwrap().activity_interval_secs };
                    if interval > 0 && beat % (interval / 5).max(1) == 0 {
                        let fg = platform::foreground_window();
                        tracing::debug!(beat, interval, fg = ?fg, "activity sample tick");
                        if let Some((app, title)) = fg {
                            let url = url_from_title(&title);
                            let sample = ActivitySample {
                                ts: chrono::Utc::now().timestamp(),
                                app: app.clone(),
                                title: title.clone(),
                                url: url.clone(),
                            };
                            if out
                                .send(ClientMessage::Activity { samples: vec![sample] })
                                .await
                                .is_err()
                            {
                                break;
                            }
                            let alert = scan_with(policy, &app, &title, url.as_deref());
                            if let Some(alert) = alert {
                                tracing::warn!(category = %alert.category, keyword = %alert.keyword, "keyword match");
                                if out.send(ClientMessage::Alert(alert)).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }
                _ = inventory.tick() => {
                    let inv = {
                        let mut sys = self.system.lock().unwrap();
                        crate::inventory::collect(&mut sys)
                    }; // guard dropped before the await
                    if out.send(ClientMessage::Inventory(Box::new(inv))).await.is_err() {
                        break;
                    }
                }
                frame = ws_rx.next() => {
                    match frame {
                        Some(Ok(Message::Text(t))) => {
                            match serde_json::from_str::<ServerMessage>(&t) {
                                Ok(ServerMessage::Command { command_id, command }) => {
                                    handle_command(ctx, command).await;
                                    out.send(ClientMessage::Ack {
                                        command_id,
                                        ok: true,
                                        detail: None,
                                    }).await.ok();
                                }
                                Ok(ServerMessage::Policy(pol)) => {
                                    apply_policy(&pol, policy);
                                }
                                Ok(ServerMessage::Chat(chat)) => {
                                    overlay::send(overlay::Update::Message {
                                        text: format!("Teacher: {}", chat.body),
                                        ttl_secs: 30,
                                    });
                                }
                                _ => {}
                            }
                        }
                        Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return Ok(()),
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }

    fn heartbeat(&self) -> Option<Heartbeat> {
        let (cpu, mem_used, mem_total) = {
            let mut sys = self.system.lock().unwrap();
            sys.refresh_cpu_all();
            sys.refresh_memory();
            (
                sys.global_cpu_usage(),
                sys.used_memory() / 1_048_576,
                sys.total_memory() / 1_048_576,
            )
        };
        let batt = platform::battery();
        Some(Heartbeat {
            ts: chrono::Utc::now().timestamp(),
            cpu_pct: cpu,
            mem_used_mb: mem_used,
            mem_total_mb: mem_total,
            battery_pct: batt.map(|(p, _)| p),
            charging: batt.map(|(_, c)| c),
            active_user: platform::active_user(),
            screen_locked: platform::is_locked() || overlay::state::blanked(),
            primary_ip: platform::primary_ip(),
            uptime_secs: System::uptime(),
        })
    }
}

/// Scanner lives beside the policy so keyword library pushes are live.
static SCANNER: Mutex<Option<Scanner>> = Mutex::new(None);

fn scan_with(policy: &Arc<Mutex<Policy>>, app: &str, title: &str, url: Option<&str>) -> Option<protocol::KeywordAlert> {
    let mut slot = SCANNER.lock().unwrap();
    if slot.is_none() {
        let pol = policy.lock().unwrap().clone();
        let mut s = Scanner::new();
        s.set_rules(
            pol.keywords
                .iter()
                .filter(|k| {
                    pol.keyword_categories.is_empty()
                        || pol.keyword_categories.contains(&k.category)
                })
                .cloned()
                .collect(),
            pol.keyword_categories.clone(),
        );
        *slot = Some(s);
    }
    slot.as_ref().unwrap().scan(app, title, url)
}

pub fn apply_policy(pol: &Policy, slot: &Arc<Mutex<Policy>>) {
    let mut prev = slot.lock().unwrap().clone();
    *slot.lock().unwrap() = pol.clone();

    overlay::send(overlay::Update::Banner(if pol.monitor_banner {
        Some(pol.school_name.clone())
    } else {
        None
    }));

    if pol.blanked != prev.blanked {
        overlay::send(overlay::Update::Blank(pol.blanked));
    }

    if pol.muted != prev.muted {
        if let Err(e) = platform::set_muted(pol.muted) {
            tracing::warn!(error = %e, "mute toggle failed");
        }
    }

    if pol.locked && !prev.locked {
        match platform::lock_screen() {
            Ok(()) => tracing::info!("screen locked by policy"),
            Err(e) => tracing::warn!(error = %e, "lock failed (permissions?)"),
        }
    }
    prev.locked = pol.locked;

    // (Re)arm the scanner with the pushed library.
    {
        let mut slot = SCANNER.lock().unwrap();
        let mut s = Scanner::new();
        s.set_rules(
            pol.keywords
                .iter()
                .filter(|k| {
                    pol.keyword_categories.is_empty()
                        || pol.keyword_categories.contains(&k.category)
                })
                .cloned()
                .collect(),
            pol.keyword_categories.clone(),
        );
        *slot = Some(s);
    }

    tracing::info!(
        locked = pol.locked,
        blanked = pol.blanked,
        muted = pol.muted,
        banner = pol.monitor_banner,
        keywords = pol.keywords.len(),
        "policy applied"
    );
}

async fn handle_command(ctx: &AgentCtx, command: Command) {
    match command {
        Command::Screenshot { max_width } => {
            if ctx.policy.lock().unwrap().capture_indicator {
                overlay::send(overlay::Update::Message {
                    text: "Screen capture taken by staff".into(),
                    ttl_secs: 3,
                });
            }
            let width = max_width.max(160);
            match tokio::task::spawn_blocking(move || platform::capture_jpeg(width)).await {
                Ok(Ok((jpeg, w, h))) => {
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&jpeg);
                    ctx.out
                        .send(ClientMessage::Screenshot {
                            device_id: ctx.device_id.clone(),
                            ts: chrono::Utc::now().timestamp(),
                            jpeg_b64: b64,
                            width: w,
                            height: h,
                        })
                        .await
                        .ok();
                }
                Ok(Err(e)) => ctx.log("warn", &format!("screenshot failed: {e}")).await,
                Err(e) => ctx.log("error", &format!("screenshot join: {e}")).await,
            }
        }
        Command::Message { text, ttl_secs } => {
            overlay::send(overlay::Update::Message { text, ttl_secs });
        }
        Command::ClearMessage => overlay::send(overlay::Update::ClearMessage),
        Command::OpenUrl { url } => {
            if let Err(e) = platform::open_url(&url) {
                ctx.log("warn", &format!("open_url failed: {e}")).await;
            }
        }
        Command::Shutdown { delay_secs } => {
            if let Err(e) = platform::power(PowerAction::Shutdown(delay_secs)) {
                ctx.log("error", &format!("shutdown failed: {e}")).await;
            }
        }
        Command::Restart { delay_secs } => {
            if let Err(e) = platform::power(PowerAction::Restart(delay_secs)) {
                ctx.log("error", &format!("restart failed: {e}")).await;
            }
        }
        Command::Logoff => {
            if let Err(e) = platform::power(PowerAction::Logoff) {
                ctx.log("error", &format!("logoff failed: {e}")).await;
            }
        }
        Command::SendFile { name, jpeg_b64_data } => match save_file(ctx, &name, &jpeg_b64_data) {
            Ok(path) => {
                overlay::send(overlay::Update::Message {
                    text: format!("File received: {name}"),
                    ttl_secs: 10,
                });
                ctx.log("info", &format!("file saved to {}", path.display()))
                    .await;
            }
            Err(e) => ctx.log("error", &format!("file save failed: {e}")).await,
        },
        Command::RescanInventory => {
            // Next inventory tick will pick it up; force one now.
            let inv = {
                let mut sys = System::new();
                crate::inventory::collect(&mut sys)
            }; // System dropped before the await (not Send on Windows)
            ctx.out
                .send(ClientMessage::Inventory(Box::new(inv)))
                .await
                .ok();
        }
        Command::Nop => {}
    }
}

fn save_file(ctx: &AgentCtx, name: &str, b64: &str) -> Result<std::path::PathBuf> {
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(anyhow!("refusing unsafe filename"));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .context("bad base64 payload")?;
    let dir = if ctx.downloads.exists() {
        ctx.downloads.clone()
    } else {
        home_dir()
    };
    std::fs::create_dir_all(&dir)?;
    let mut path = dir.join(name);
    if path.exists() {
        path = dir.join(format!("{}-{name}", chrono::Utc::now().timestamp()));
    }
    std::fs::write(&path, bytes)?;
    Ok(path)
}

/// Convert an http(s) base URL into a ws(s) WebSocket URL.
pub fn to_ws_url(base: &str, path: &str) -> String {
    let b = base.trim_end_matches('/');
    let converted = if let Some(rest) = b.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = b.strip_prefix("http://") {
        format!("ws://{rest}")
    } else if b.starts_with("ws://") || b.starts_with("wss://") {
        b.to_string()
    } else {
        format!("ws://{b}")
    };
    format!("{converted}{path}")
}

/// One-shot helper for CLI subcommands (raise hand / chat).
pub async fn send_one_shot(identity: &Identity, msg: ClientMessage) -> Result<()> {
    let url = to_ws_url(&identity.server, "/ws/device");
    let (ws, _) = tokio_tungstenite::connect_async(&url).await?;
    let (mut tx, mut rx) = ws.split();
    let hello = ClientMessage::Hello(Hello {
        protocol: protocol::PROTOCOL_VERSION,
        device_id: identity.device_id.clone(),
        token: identity.device_token.clone(),
        agent_version: protocol::AGENT_VERSION.to_string(),
    });
    tx.send(Message::Text(serde_json::to_string(&hello)?.into())).await?;
    let first = rx.next().await.ok_or_else(|| anyhow!("no hello response"))??;
    match serde_json::from_str::<ServerMessage>(first.to_text()?)? {
        ServerMessage::HelloOk { .. } => {}
        ServerMessage::HelloDenied { reason } => return Err(anyhow!("denied: {reason}")),
        _ => {}
    }
    tx.send(Message::Text(serde_json::to_string(&msg)?.into())).await?;
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let _ = tx.close().await;
    Ok(())
}

pub fn new_raise_hand(note: Option<String>) -> ClientMessage {
    ClientMessage::RaiseHand(protocol::RaiseHand {
        ts: chrono::Utc::now().timestamp(),
        note,
    })
}

pub fn new_chat(body: String) -> ClientMessage {
    ClientMessage::Chat(ChatMessage {
        ts: chrono::Utc::now().timestamp(),
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_url_converts_schemes() {
        assert_eq!(to_ws_url("http://host:1/", "/ws/device"), "ws://host:1/ws/device");
        assert_eq!(to_ws_url("https://host", "/ws/device"), "wss://host/ws/device");
        assert_eq!(to_ws_url("wss://host", "/ws/device"), "wss://host/ws/device");
        assert_eq!(to_ws_url("host", "/ws/device"), "ws://host/ws/device");
    }
}
