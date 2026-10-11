//! WebSocket endpoints: `/ws/device` (agent channel) and `/ws/console` (live feed).

use crate::state::Shared;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use protocol::{ClientMessage, Command, Hello, ServerMessage};
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::mpsc;

pub async fn device_ws(
    State(state): State<Shared>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| device_loop(state, socket, headers))
}

async fn device_loop(state: Shared, socket: WebSocket, headers: HeaderMap) {
    let (mut tx, mut rx) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<ServerMessage>(64);

    // First frame must be hello within 10s, with token also accepted via
    // Authorization: Bearer for transports that can't set headers on WS.
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").map(|s| s.to_string()));

    let hello = tokio::time::timeout(Duration::from_secs(10), rx.next()).await;
    let device_id = match hello {
        Ok(Some(Ok(Message::Text(text)))) => {
            let parsed: Option<ClientMessage> = serde_json::from_str(&text).ok();
            match parsed {
                Some(ClientMessage::Hello(h)) => {
                    let token = if h.token.is_empty() {
                        bearer.unwrap_or_default()
                    } else {
                        h.token.clone()
                    };
                    match authorize(&state, &h, &token).await {
                        Ok(id) => id,
                        Err(reason) => {
                            let msg = serde_json::to_string(&ServerMessage::HelloDenied {
                                reason: reason.clone(),
                            })
                            .unwrap_or_default();
                            let _ = tx.send(Message::Text(msg.into())).await;
                            tracing::warn!(device_id = %h.device_id, %reason, "device denied");
                            return;
                        }
                    }
                }
                _ => return,
            }
        }
        _ => return,
    };

    tracing::info!(%device_id, "device connected");
    state.register_device(&device_id, out_tx.clone()).await;

    // Deliver anything queued while offline, then the current policy.
    let mut queued: Vec<Command> = Vec::new();
    if let Ok(rows) = state.db.take_queued_commands(&device_id) {
        for (id, json) in rows {
            if let Ok(cmd) = serde_json::from_str::<Command>(&json) {
                queued.push(cmd);
                let _ = state.db.mark_command(&id, "sent", None);
            }
        }
    }
    let policy = state
        .db
        .device(&device_id, state.config.offline_after_secs)
        .ok()
        .flatten()
        .and_then(|d| Some(d.policy))
        .unwrap_or_default();

    let hello_ok = ServerMessage::HelloOk {
        server_version: protocol::SERVER_VERSION.to_string(),
        policy,
        queued,
    };
    if out_tx.send(hello_ok).await.is_err() {
        state.unregister_device(&device_id).await;
        return;
    }

    // Fan-out task: server -> device
    let fanout_device = device_id.clone();
    let state2 = state.clone();
    let fanout = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            let text = match serde_json::to_string(&msg) {
                Ok(t) => t,
                Err(_) => continue,
            };
            if tx.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
        state2.unregister_device(&fanout_device).await;
    });

    // Read loop: device -> server
    while let Some(frame) = rx.next().await {
        let text = match frame {
            Ok(Message::Text(t)) => t.to_string(),
            Ok(Message::Close(_)) | Err(_) => break,
            _ => continue,
        };
        let msg: ClientMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                tracing::debug!(%e, "unparseable client frame");
                continue;
            }
        };
        if let Err(e) = handle_client(&state, &device_id, msg).await {
            tracing::warn!(%device_id, error = %e, "client message failed");
        }
    }

    fanout.abort();
    state.unregister_device(&device_id).await;
    tracing::info!(%device_id, "device disconnected");
}

async fn authorize(state: &Shared, hello: &Hello, token: &str) -> Result<String, String> {
    if hello.protocol != protocol::PROTOCOL_VERSION {
        return Err(format!(
            "protocol mismatch: server speaks v{}, agent speaks v{}",
            protocol::PROTOCOL_VERSION,
            hello.protocol
        ));
    }
    if token.is_empty() {
        return Err("missing device token".into());
    }
    if state
        .db
        .device_token_valid(&hello.device_id, token)
        .map_err(|e| e.to_string())?
    {
        Ok(hello.device_id.clone())
    } else {
        Err("invalid device credentials; re-enroll the agent".into())
    }
}

async fn handle_client(state: &Shared, device_id: &str, msg: ClientMessage) -> anyhow::Result<()> {
    match msg {
        ClientMessage::Heartbeat(hb) => {
            state.db.update_heartbeat(
                device_id,
                &hb.active_user,
                &hb.primary_ip,
                hb.cpu_pct,
                hb.mem_used_mb,
                hb.mem_total_mb,
                hb.battery_pct,
                hb.screen_locked,
                hb.uptime_secs,
            )?;
        }
        ClientMessage::Inventory(inv) => {
            state.db.update_inventory(device_id, &serde_json::to_string(&inv)?)?;
            state.notify_console(
                "device",
                serde_json::json!({ "id": device_id, "change": "inventory" }),
            );
        }
        ClientMessage::Activity { samples } => {
            for s in samples {
                let detail = serde_json::json!({
                    "app": s.app, "title": s.title, "url": s.url
                })
                .to_string();
                state
                    .db
                    .add_event(device_id, "activity", "info", &s.app, &detail)?;
            }
        }
        ClientMessage::Alert(alert) => {
            let detail = serde_json::json!({
                "category": alert.category,
                "keyword": alert.keyword,
                "context": alert.context,
                "source": format!("{:?}", alert.source),
                "app": alert.app,
                "evidence_ref": alert.evidence_ref,
            })
            .to_string();
            let id = state.db.add_event(
                device_id,
                "alert",
                &format!("{:?}", alert.severity).to_lowercase(),
                &format!("{}: “{}”", alert.category, alert.keyword),
                &detail,
            )?;
            // Real-time feed to consoles.
            let (name, _) = device_display(state, device_id).await;
            let row = serde_json::json!({
                "id": id, "ts": alert.ts, "device_id": device_id, "device_name": name,
                "kind": "alert",
                "severity": format!("{:?}", alert.severity).to_lowercase(),
                "title": format!("{}: “{}”", alert.category, alert.keyword),
                "detail": detail,
            });
            state.notify_console("alert", row);
        }
        ClientMessage::Screenshot {
            ts,
            jpeg_b64,
            width,
            height,
            ..
        } => {
            let bytes = base64::Engine::decode(
                &base64::engine::general_purpose::STANDARD,
                &jpeg_b64,
            )?;
            let dir = state
                .config
                .data_dir
                .join("screenshots")
                .join(device_id);
            std::fs::create_dir_all(&dir)?;
            let name = format!("{ts}.jpg");
            std::fs::write(dir.join(&name), bytes)?;
            state.db.set_thumbnail_ts(device_id, ts)?;
            state.notify_console(
                "thumbnail",
                serde_json::json!({ "device_id": device_id, "ts": ts,
                                     "width": width, "height": height }),
            );
        }
        ClientMessage::RaiseHand(rh) => {
            let detail = rh.note.unwrap_or_default();
            let id = state.db.add_event(
                device_id,
                "raise_hand",
                "info",
                "Student raised hand",
                &detail,
            )?;
            let (name, _) = device_display(state, device_id).await;
            state.notify_console(
                "raise_hand",
                serde_json::json!({ "id": id, "ts": rh.ts, "device_id": device_id,
                                     "device_name": name, "note": detail }),
            );
        }
        ClientMessage::Chat(chat) => {
            let id = state.db.add_event(device_id, "chat", "info", "Student message", &chat.body)?;
            let (name, _) = device_display(state, device_id).await;
            state.notify_console(
                "chat",
                serde_json::json!({ "id": id, "ts": chat.ts, "device_id": device_id,
                                     "device_name": name, "body": chat.body }),
            );
        }
        ClientMessage::Ack {
            command_id,
            ok,
            detail,
        } => {
            state.db.mark_command(
                &command_id,
                if ok { "done" } else { "failed" },
                detail.as_deref(),
            )?;
        }
        ClientMessage::Log { level, message, .. } => {
            state.db.add_event(device_id, "agent_log", &level, "Agent log", &message)?;
        }
        ClientMessage::Hello(_) => {
            // Repeated hello on a live channel: resend policy as convergence.
            if let Ok(Some(dev)) = state.db.device(device_id, state.config.offline_after_secs) {
                state
                    .send_device(device_id, ServerMessage::Policy(dev.policy))
                    .await;
            }
        }
    }
    Ok(())
}

async fn device_display(state: &Shared, device_id: &str) -> (String, String) {
    match state
        .db
        .device(device_id, state.config.offline_after_secs)
    {
        Ok(Some(d)) => (d.name, d.active_user),
        _ => (device_id.to_string(), String::new()),
    }
}

// ---------------------------------------------------------------------------
// Console websocket
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ConsoleQuery {
    pub token: Option<String>,
}

pub async fn console_ws(
    State(state): State<Shared>,
    Query(q): Query<ConsoleQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    let token = q.token.unwrap_or_default();
    match state.db.session(&token) {
        Ok(Some((user, _role))) => {
            tracing::info!(%user, "console connected");
            ws.on_upgrade(move |socket| console_loop(state, socket))
        }
        _ => axum::http::StatusCode::UNAUTHORIZED.into_response(),
    }
}

async fn console_loop(state: Shared, socket: WebSocket) {
    let (mut tx, mut rx) = socket.split();
    let mut events = state.console_tx.subscribe();
    let mut ping = tokio::time::interval(Duration::from_secs(20));

    loop {
        tokio::select! {
            evt = events.recv() => {
                match evt {
                    Ok(text) => {
                        if tx.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            _ = ping.tick() => {
                if tx.send(Message::Ping(vec![].into())).await.is_err() {
                    break;
                }
            }
            frame = rx.next() => {
                match frame {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}

