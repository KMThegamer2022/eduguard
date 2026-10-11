//! REST API for the console and for enrollment.

use crate::config::random_token;
use crate::state::Shared;
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use protocol::{Command, EnrollmentRequest, EnrollmentResponse, LoginRequest, Policy, ServerMessage};
use serde::Deserialize;
use serde_json::json;

pub fn router() -> Router<Shared> {
    Router::new()
        .route("/api/enroll", post(enroll))
        .route("/api/login", post(login))
        .route("/api/me", get(me))
        .route("/api/stats", get(stats))
        .route("/api/devices", get(devices))
        .route("/api/devices/{id}", get(device))
        .route("/api/devices/{id}/inventory", get(device_inventory))
        .route("/api/devices/{id}/thumbnail", get(thumbnail))
        .route("/api/devices/{id}/command", post(device_command))
        .route("/api/devices/{id}/policy", put(device_policy))
        .route("/api/policy", get(template_policy).put(put_template_policy))
        .route("/api/keywords", get(keywords).put(put_keywords))
        .route("/api/events", get(events))
        .route("/api/events/{id}/status", put(event_status))
        .route("/api/audit", get(audit))
        .route("/api/enrollment-tokens", get(list_tokens).post(create_token))
        .route("/api/enrollment-tokens/{token}", axum::routing::delete(delete_token))
        .route("/api/files", post(send_file))
        .route("/api/viewing", post(viewing))
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024))
}

// ---------------------------------------------------------------------------
// auth helpers
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Actor {
    pub username: String,
    pub role: String,
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").map(|s| s.to_string()))
}

async fn auth(state: &Shared, headers: &HeaderMap) -> Result<Actor, Response> {
    let token = bearer(headers).ok_or_else(unauthorized)?;
    state
        .db
        .session(&token)
        .ok()
        .flatten()
        .map(|(username, role)| Actor { username, role })
        .ok_or_else(unauthorized)
}

/// Image subresources (CSS `background-image` device previews) cannot carry
/// an `Authorization` header, so [`thumbnail`] also accepts the session token
/// as `?token=...`. It is validated against the session table exactly like a
/// Bearer credential.
fn query_token(state: &Shared, uri: &axum::http::Uri) -> bool {
    let Some(q) = uri.query() else { return false };
    q.split('&').any(|kv| {
        kv.strip_prefix("token=")
            .filter(|v| !v.is_empty())
            .map(|v| matches!(state.db.session(v), Ok(Some(_))))
            .unwrap_or(false)
    })
}

async fn require_admin(actor: &Actor) -> Result<(), Response> {
    if actor.role == "admin" {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "admin role required" })),
        )
            .into_response())
    }
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({ "error": "unauthorized" }))).into_response()
}

fn internal(e: impl std::fmt::Display) -> Response {
    tracing::error!(error = %e, "internal error");
    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response()
}

fn bad(msg: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": msg }))).into_response()
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response()
}

// ---------------------------------------------------------------------------
// enrollment + login
// ---------------------------------------------------------------------------

/// Light per-process throttle against enrollment-token guessing.
static ENROLL_GUARD: std::sync::Mutex<(i64, u32)> = std::sync::Mutex::new((0, 0));

async fn enroll(State(state): State<Shared>, Json(req): Json<EnrollmentRequest>) -> Response {
    // Rate limit: 30 attempts / 10 minutes / process.
    {
        let mut g = ENROLL_GUARD.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        if now - g.0 > 600 {
            *g = (now, 0);
        }
        g.1 += 1;
        if g.1 > 30 {
            return (StatusCode::TOO_MANY_REQUESTS, Json(json!({ "error": "rate limited" })))
                .into_response();
        }
    }

    if !state.db.valid_enrollment_token(&req.enrollment_token).unwrap_or(false) {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "invalid enrollment token" })))
            .into_response();
    }

    let device_id = format!("dev-{}", random_token(8));
    let device_token = random_token(24);
    let name = req
        .device_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| req.hostname.clone());

    let mut policy: Policy = state
        .db
        .get_setting("policy_template")
        .ok()
        .flatten()
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();
    if policy.school_name.is_empty() || policy.school_name == "EduGuard School" {
        policy.school_name = state.config.school_name.clone();
    }

    if let Err(e) = state.db.create_device(
        &device_id,
        &name,
        &req.hostname,
        &req.os,
        &req.os_version,
        &req.arch,
        &req.agent_version,
        &device_token,
        &policy,
    ) {
        return internal(e);
    }
    let _ = state.db.bump_enrollment_token(&req.enrollment_token);
    let _ = state.db.audit(
        &req.enrolled_by.clone().unwrap_or_else(|| "agent".into()),
        "device.enroll",
        &device_id,
        &format!("{} {} {}", req.hostname, req.os, req.arch),
    );
    state.notify_console(
        "device",
        json!({ "id": device_id, "change": "enrolled" }),
    );

    Json(EnrollmentResponse {
        device_id,
        device_token,
        server_version: protocol::SERVER_VERSION.to_string(),
    })
    .into_response()
}

async fn login(State(state): State<Shared>, Json(req): Json<LoginRequest>) -> Response {
    let cfg = &state.config;
    let ok_user = req.username == cfg.admin_username;
    // Compare without early-exit on the password to avoid trivial timing skew.
    let ok_pass = !cfg.admin_password.is_empty() && req.password == cfg.admin_password;
    if !(ok_user && ok_pass) {
        // Teacher accounts: admin can create later; for now console uses admin.
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "invalid credentials" })))
            .into_response();
    }
    let token = random_token(24);
    if let Err(e) = state
        .db
        .create_session(&token, &req.username, "admin", cfg.session_ttl_secs)
    {
        return internal(e);
    }
    let _ = state
        .db
        .audit(&req.username, "console.login", &req.username, "web console");
    Json(protocol::LoginResponse {
        token,
        username: req.username,
        role: "admin".into(),
    })
    .into_response()
}

async fn me(State(state): State<Shared>, headers: HeaderMap) -> Response {
    match auth(&state, &headers).await {
        Ok(a) => Json(json!({ "username": a.username, "role": a.role })).into_response(),
        Err(r) => r,
    }
}

// ---------------------------------------------------------------------------
// devices
// ---------------------------------------------------------------------------

async fn stats(State(state): State<Shared>, headers: HeaderMap) -> Response {
    if auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    match state.db.stats(state.config.offline_after_secs) {
        Ok(v) => Json(v).into_response(),
        Err(e) => internal(e),
    }
}

async fn devices(State(state): State<Shared>, headers: HeaderMap) -> Response {
    if auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    match state.db.devices(state.config.offline_after_secs) {
        Ok(list) => Json(list).into_response(),
        Err(e) => internal(e),
    }
}

async fn device(
    State(state): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    match state.db.device(&id, state.config.offline_after_secs) {
        Ok(Some(d)) => Json(d).into_response(),
        Ok(None) => not_found(),
        Err(e) => internal(e),
    }
}

async fn device_inventory(
    State(state): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    match state.db.inventory_json(&id) {
        Ok(Some(j)) => {
            let v: serde_json::Value = serde_json::from_str(&j).unwrap_or(json!({}));
            Json(v).into_response()
        }
        Ok(None) => not_found(),
        Err(e) => internal(e),
    }
}

async fn thumbnail(
    State(state): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> Response {
    if auth(&state, &headers).await.is_err() && !query_token(&state, &uri) {
        return unauthorized();
    }
    state.mark_viewing();
    // Serve newest thumbnail; fall back to the newest file on disk.
    let dir = state.config.data_dir.join("screenshots").join(&id);
    let mut candidates: Vec<i64> = Vec::new();
    if let Ok(dev) = state.db.device(&id, state.config.offline_after_secs) {
        if let Some(d) = dev {
            if let Some(ts) = d.thumbnail_ts {
                candidates.push(ts);
            }
        }
    }
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if let Some(stem) = name.strip_suffix(".jpg") {
                    if let Ok(ts) = stem.parse::<i64>() {
                        candidates.push(ts);
                    }
                }
            }
        }
    }
    candidates.sort_unstable();
    for ts in candidates.into_iter().rev() {
        let path = dir.join(format!("{ts}.jpg"));
        if let Ok(bytes) = std::fs::read(&path) {
            return (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "image/jpeg".to_string()),
                    (header::CACHE_CONTROL, "no-store".to_string()),
                ],
                bytes,
            )
                .into_response();
        }
    }
    not_found()
}

#[derive(Deserialize)]
struct CommandRequest {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    ttl_secs: Option<u64>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    delay_secs: Option<u64>,
    #[serde(default)]
    max_width: Option<u32>,
}

async fn device_command(
    State(state): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<CommandRequest>,
) -> Response {
    let actor = match auth(&state, &headers).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    let Some(dev) = state
        .db
        .device(&id, state.config.offline_after_secs)
        .ok()
        .flatten()
    else {
        return not_found();
    };

    let command: Command = match req.kind.as_str() {
        "screenshot" => Command::Screenshot {
            max_width: req.max_width.unwrap_or(320),
        },
        "message" => Command::Message {
            text: req.text.clone().unwrap_or_default(),
            ttl_secs: req.ttl_secs.unwrap_or(15),
        },
        "clear_message" => Command::ClearMessage,
        "open_url" => match req.url.clone() {
            Some(url) if url.starts_with("http://") || url.starts_with("https://") => {
                Command::OpenUrl { url }
            }
            _ => return bad("url must be http(s)"),
        },
        "shutdown" => Command::Shutdown {
            delay_secs: req.delay_secs.unwrap_or(0),
        },
        "restart" => Command::Restart {
            delay_secs: req.delay_secs.unwrap_or(0),
        },
        "logoff" => Command::Logoff,
        "rescan_inventory" => Command::RescanInventory,
        other => return bad(&format!("unknown command type: {other}")),
    };

    let command_id = random_token(8);
    let json = serde_json::to_string(&command).unwrap_or_default();
    if let Err(e) = state
        .db
        .queue_command(&command_id, &id, &json, &actor.username)
    {
        return internal(e);
    }

    let delivered = state
        .send_device(
            &id,
            ServerMessage::Command {
                command_id: command_id.clone(),
                command,
            },
        )
        .await;
    if delivered {
        let _ = state.db.mark_command(&command_id, "sent", None);
    }
    let _ = state.db.audit(
        &actor.username,
        &format!("device.command.{}", req.kind),
        &dev.name,
        &format!("device={id} delivered={delivered}"),
    );
    state.notify_console(
        "command",
        json!({ "device_id": id, "kind": req.kind, "delivered": delivered }),
    );

    Json(json!({ "command_id": command_id, "delivered": delivered })).into_response()
}

async fn device_policy(
    State(state): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(mut patch): Json<Policy>,
) -> Response {
    let actor = match auth(&state, &headers).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    let Some(mut dev) = state
        .db
        .device(&id, state.config.offline_after_secs)
        .ok()
        .flatten()
    else {
        return not_found();
    };
    patch.school_name = dev.policy.school_name.clone();
    dev.policy = patch;
    if let Err(e) = state.db.set_device_policy(&id, &dev.policy) {
        return internal(e);
    }
    let delivered = state
        .send_device(&id, ServerMessage::Policy(dev.policy.clone()))
        .await;
    let changed = format!(
        "locked={} blanked={} muted={}",
        dev.policy.locked, dev.policy.blanked, dev.policy.muted
    );
    let _ = state
        .db
        .audit(&actor.username, "device.policy", &dev.name, &changed);
    state.notify_console(
        "policy",
        json!({ "device_id": id, "policy": dev.policy }),
    );
    Json(json!({ "ok": true, "delivered": delivered })).into_response()
}

async fn send_file(
    State(state): State<Shared>,
    headers: HeaderMap,
    mut mp: Multipart,
) -> Response {
    let actor = match auth(&state, &headers).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    let mut device_id = String::new();
    let mut filename = String::new();
    let mut data: Vec<u8> = Vec::new();
    while let Ok(Some(field)) = mp.next_field().await {
        match field.name().unwrap_or("") {
            "device_id" => device_id = field.text().await.unwrap_or_default(),
            "filename" => filename = field.text().await.unwrap_or_default(),
            "file" => data = field.bytes().await.unwrap_or_default().to_vec(),
            _ => {}
        }
    }
    if device_id.is_empty() || filename.is_empty() || data.is_empty() {
        return bad("device_id, filename and file are required");
    }
    if data.len() > 10 * 1024 * 1024 {
        return bad("file too large (10 MB max)");
    }
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return bad("bad filename");
    }
    let Some(dev) = state
        .db
        .device(&device_id, state.config.offline_after_secs)
        .ok()
        .flatten()
    else {
        return not_found();
    };
    let cmd = Command::SendFile {
        name: filename.clone(),
        jpeg_b64_data: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data),
    };
    let command_id = random_token(8);
    let json = serde_json::to_string(&cmd).unwrap_or_default();
    let _ = state
        .db
        .queue_command(&command_id, &device_id, &json, &actor.username);
    let delivered = state
        .send_device(
            &device_id,
            ServerMessage::Command {
                command_id: command_id.clone(),
                command: cmd,
            },
        )
        .await;
    if delivered {
        let _ = state.db.mark_command(&command_id, "sent", None);
    }
    let _ = state.db.audit(
        &actor.username,
        "device.send_file",
        &dev.name,
        &format!("{filename} ({} bytes)", data.len()),
    );
    Json(json!({ "command_id": command_id, "delivered": delivered })).into_response()
}

// ---------------------------------------------------------------------------
// policy template + keywords
// ---------------------------------------------------------------------------

async fn template_policy(State(state): State<Shared>, headers: HeaderMap) -> Response {
    if auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    let policy: Policy = state
        .db
        .get_setting("policy_template")
        .ok()
        .flatten()
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();
    Json(policy).into_response()
}

async fn put_template_policy(
    State(state): State<Shared>,
    headers: HeaderMap,
    Json(policy): Json<Policy>,
) -> Response {
    let actor = match auth(&state, &headers).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    if let Err(e) = require_admin(&actor).await {
        return e;
    }
    let json = serde_json::to_string_pretty(&policy).unwrap_or_default();
    if let Err(e) = state.db.set_setting("policy_template", &json) {
        return internal(e);
    }
    let _ = state.db.audit(
        &actor.username,
        "policy.template.update",
        "global",
        &format!(
            "monitor_banner={} activity={} keywords={}",
            policy.monitor_banner,
            policy.activity_interval_secs,
            policy.keywords.len()
        ),
    );
    Json(json!({ "ok": true })).into_response()
}

async fn keywords(State(state): State<Shared>, headers: HeaderMap) -> Response {
    if auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    match state.db.keywords() {
        Ok(list) => Json(list).into_response(),
        Err(e) => internal(e),
    }
}

async fn put_keywords(
    State(state): State<Shared>,
    headers: HeaderMap,
    Json(rules): Json<Vec<protocol::KeywordRule>>,
) -> Response {
    let actor = match auth(&state, &headers).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    if let Err(e) = require_admin(&actor).await {
        return e;
    }
    let json = serde_json::to_string_pretty(&rules).unwrap_or_default();
    if let Err(e) = state.db.set_setting("keywords", &json) {
        return internal(e);
    }
    // Push to every device immediately so library changes are live (M3 goal).
    let mut policy: Policy = state
        .db
        .get_setting("policy_template")
        .ok()
        .flatten()
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();
    policy.keywords = rules
        .iter()
        .filter(|k| policy.keyword_categories.is_empty()
            || policy.keyword_categories.contains(&k.category))
        .cloned()
        .collect();
    state
        .broadcast_devices(ServerMessage::Policy(policy))
        .await;
    let _ = state.db.audit(
        &actor.username,
        "policy.keywords.update",
        "global",
        &format!("{} rules", rules.len()),
    );
    Json(json!({ "ok": true })).into_response()
}

// ---------------------------------------------------------------------------
// events + audit
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    kind: Option<String>,
}

async fn events(
    State(state): State<Shared>,
    Query(q): Query<EventsQuery>,
    headers: HeaderMap,
) -> Response {
    if auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    let limit = q.limit.unwrap_or(200).clamp(1, 1000);
    match state.db.events(limit, q.kind.as_deref()) {
        Ok(list) => Json(list).into_response(),
        Err(e) => internal(e),
    }
}

#[derive(Deserialize)]
struct StatusBody {
    status: String,
}

async fn event_status(
    State(state): State<Shared>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Json(body): Json<StatusBody>,
) -> Response {
    let actor = match auth(&state, &headers).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    if !matches!(body.status.as_str(), "open" | "acknowledged" | "resolved") {
        return bad("status must be open|acknowledged|resolved");
    }
    match state.db.set_event_status(id, &body.status) {
        Ok(true) => {
            let _ = state
                .db
                .audit(&actor.username, "incident.status", &format!("event:{id}"), &body.status);
            Json(json!({ "ok": true })).into_response()
        }
        Ok(false) => not_found(),
        Err(e) => internal(e),
    }
}

async fn audit(
    State(state): State<Shared>,
    Query(q): Query<EventsQuery>,
    headers: HeaderMap,
) -> Response {
    let actor = match auth(&state, &headers).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    if let Err(r) = require_admin(&actor).await {
        return r;
    }
    let limit = q.limit.unwrap_or(300).clamp(1, 2000);
    match state.db.audit_rows(limit) {
        Ok(list) => Json(list).into_response(),
        Err(e) => internal(e),
    }
}

// ---------------------------------------------------------------------------
// enrollment tokens + viewing
// ---------------------------------------------------------------------------

async fn list_tokens(State(state): State<Shared>, headers: HeaderMap) -> Response {
    let actor = match auth(&state, &headers).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    if let Err(r) = require_admin(&actor).await {
        return r;
    }
    match state.db.list_enrollment_tokens() {
        Ok(rows) => {
            let list: Vec<_> = rows
                .into_iter()
                .map(|(token, label, created, expires, used)| {
                    json!({
                        "token": token, "label": label, "created_at": created,
                        "expires_at": expires, "used_count": used,
                    })
                })
                .collect();
            Json(list).into_response()
        }
        Err(e) => internal(e),
    }
}

#[derive(Deserialize)]
struct CreateToken {
    #[serde(default)]
    label: String,
    #[serde(default)]
    expires_in_secs: Option<u64>,
}

async fn create_token(
    State(state): State<Shared>,
    headers: HeaderMap,
    Json(req): Json<CreateToken>,
) -> Response {
    let actor = match auth(&state, &headers).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    if let Err(r) = require_admin(&actor).await {
        return r;
    }
    let token = format!("eg-{}", random_token(16));
    let expires = req
        .expires_in_secs
        .map(|s| chrono::Utc::now().timestamp() + s as i64);
    if let Err(e) = state.db.add_enrollment_token(&token, &req.label, expires) {
        return internal(e);
    }
    let _ = state
        .db
        .audit(&actor.username, "enrollment_token.create", &token, &req.label);
    Json(json!({ "token": token })).into_response()
}

async fn delete_token(
    State(state): State<Shared>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> Response {
    let actor = match auth(&state, &headers).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    if let Err(r) = require_admin(&actor).await {
        return r;
    }
    match state.db.delete_enrollment_token(&token) {
        Ok(true) => {
            let _ = state
                .db
                .audit(&actor.username, "enrollment_token.delete", &token, "");
            Json(json!({ "ok": true })).into_response()
        }
        Ok(false) => not_found(),
        Err(e) => internal(e),
    }
}

#[derive(Deserialize)]
struct ViewingBody {
    active: bool,
}

async fn viewing(State(state): State<Shared>, headers: HeaderMap, Json(b): Json<ViewingBody>) -> Response {
    if auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    if b.active {
        state.mark_viewing();
    }
    Json(json!({ "ok": true })).into_response()
}
