//! SQLite persistence: devices, events, commands, audit, sessions, settings.

use anyhow::{anyhow, Context, Result};
use protocol::{Device, KeywordRule, Policy};
use rusqlite::{params, Connection, OptionalExtension, Row};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Mutex;

pub fn hash(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS devices (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    hostname TEXT NOT NULL,
    os TEXT NOT NULL,
    os_version TEXT NOT NULL,
    arch TEXT NOT NULL,
    agent_version TEXT NOT NULL,
    enrolled_at INTEGER NOT NULL,
    token_hash TEXT NOT NULL,
    active_user TEXT NOT NULL DEFAULT '',
    primary_ip TEXT NOT NULL DEFAULT '',
    cpu_pct REAL NOT NULL DEFAULT 0,
    mem_used_mb INTEGER NOT NULL DEFAULT 0,
    mem_total_mb INTEGER NOT NULL DEFAULT 0,
    battery_pct REAL,
    screen_locked INTEGER NOT NULL DEFAULT 0,
    uptime_secs INTEGER NOT NULL DEFAULT 0,
    last_seen INTEGER,
    thumbnail_ts INTEGER,
    inventory_json TEXT NOT NULL DEFAULT '{}',
    policy_json TEXT NOT NULL DEFAULT '{}'
);
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts INTEGER NOT NULL,
    device_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    severity TEXT NOT NULL,
    title TEXT NOT NULL,
    detail TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'open'
);
CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts DESC);
CREATE INDEX IF NOT EXISTS idx_events_kind ON events(kind);
CREATE TABLE IF NOT EXISTS commands (
    id TEXT PRIMARY KEY,
    ts INTEGER NOT NULL,
    device_id TEXT NOT NULL,
    command_json TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    result TEXT,
    actor TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts INTEGER NOT NULL,
    actor TEXT NOT NULL,
    action TEXT NOT NULL,
    target TEXT NOT NULL,
    detail TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS enrollment_tokens (
    token TEXT PRIMARY KEY,
    label TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    expires_at INTEGER,
    used_count INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS sessions (
    token_hash TEXT PRIMARY KEY,
    username TEXT NOT NULL,
    role TEXT NOT NULL,
    expires INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.pragma_update(None, "synchronous", "NORMAL").ok();
        conn.pragma_update(None, "foreign_keys", "ON").ok();
        conn.execute_batch(SCHEMA).context("creating schema")?;
        Ok(Db {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Db {
            conn: Mutex::new(conn),
        })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock().expect("db lock poisoned");
        f(&conn)
    }

    // ---- settings ---------------------------------------------------------

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        self.with(|c| {
            Ok(c.query_row(
                "SELECT value FROM settings WHERE key=?1",
                params![key],
                |r| r.get::<_, String>(0),
            )
            .optional()?)
        })
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO settings(key,value) VALUES(?1,?2)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, value],
            )?;
            Ok(())
        })
    }

    // ---- devices ----------------------------------------------------------

    pub fn create_device(
        &self,
        id: &str,
        name: &str,
        hostname: &str,
        os: &str,
        os_version: &str,
        arch: &str,
        agent_version: &str,
        token: &str,
        policy: &Policy,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        let policy_json = serde_json::to_string(policy)?;
        self.with(|c| {
            c.execute(
                "INSERT INTO devices(id,name,hostname,os,os_version,arch,agent_version,
                     enrolled_at,token_hash,policy_json)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    id,
                    name,
                    hostname,
                    os,
                    os_version,
                    arch,
                    agent_version,
                    now,
                    hash(token),
                    policy_json
                ],
            )?;
            Ok(())
        })
    }

    pub fn device_token_valid(&self, id: &str, token: &str) -> Result<bool> {
        self.with(|c| {
            let found: Option<String> = c
                .query_row(
                    "SELECT token_hash FROM devices WHERE id=?1",
                    params![id],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(found.map(|h| h == hash(token)).unwrap_or(false))
        })
    }

    fn row_to_device(r: &Row, online_after: i64) -> rusqlite::Result<Device> {
        let policy_json: String = r.get("policy_json")?;
        let thumb: Option<i64> = r.get("thumbnail_ts")?;
        Ok(Device {
            id: r.get("id")?,
            name: r.get("name")?,
            hostname: r.get("hostname")?,
            os: r.get("os")?,
            os_version: r.get("os_version")?,
            arch: r.get("arch")?,
            agent_version: r.get("agent_version")?,
            enrolled_at: r.get("enrolled_at")?,
            last_seen: r.get("last_seen")?,
            online: r
                .get::<_, Option<i64>>("last_seen")?
                .map(|t| t >= online_after)
                .unwrap_or(false),
            active_user: r.get("active_user")?,
            primary_ip: r.get("primary_ip")?,
            cpu_pct: r.get("cpu_pct")?,
            mem_used_mb: r.get::<_, i64>("mem_used_mb")? as u64,
            mem_total_mb: r.get::<_, i64>("mem_total_mb")? as u64,
            battery_pct: r.get("battery_pct")?,
            screen_locked: r.get::<_, i64>("screen_locked")? != 0,
            thumbnail_ts: thumb,
            policy: serde_json::from_str(&policy_json).unwrap_or_default(),
        })
    }

    pub fn devices(&self, offline_after_secs: u64) -> Result<Vec<Device>> {
        let cutoff = chrono::Utc::now().timestamp() - offline_after_secs as i64;
        self.with(|c| {
            let mut stmt =
                c.prepare("SELECT * FROM devices ORDER BY name COLLATE NOCASE")?;
            let rows = stmt.query_map([], |r| Self::row_to_device(r, cutoff))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
    }

    pub fn device(&self, id: &str, offline_after_secs: u64) -> Result<Option<Device>> {
        let cutoff = chrono::Utc::now().timestamp() - offline_after_secs as i64;
        self.with(|c| {
            Ok(c.query_row("SELECT * FROM devices WHERE id=?1", params![id], |r| {
                Self::row_to_device(r, cutoff)
            })
            .optional()?)
        })
    }

    pub fn update_heartbeat(
        &self,
        id: &str,
        active_user: &str,
        primary_ip: &str,
        cpu_pct: f32,
        mem_used_mb: u64,
        mem_total_mb: u64,
        battery_pct: Option<f32>,
        screen_locked: bool,
        uptime_secs: u64,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.with(|c| {
            c.execute(
                "UPDATE devices SET last_seen=?2, active_user=?3, primary_ip=?4,
                     cpu_pct=?5, mem_used_mb=?6, mem_total_mb=?7, battery_pct=?8,
                     screen_locked=?9, uptime_secs=?10
                 WHERE id=?1",
                params![
                    id,
                    now,
                    active_user,
                    primary_ip,
                    cpu_pct as f64,
                    mem_used_mb as i64,
                    mem_total_mb as i64,
                    battery_pct.map(f64::from),
                    screen_locked as i64,
                    uptime_secs as i64
                ],
            )?;
            Ok(())
        })
    }

    pub fn update_inventory(&self, id: &str, inventory_json: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE devices SET inventory_json=?2, agent_version=(
                     COALESCE(json_extract(?2,'$.agent_version'), agent_version))
                 WHERE id=?1",
                params![id, inventory_json],
            )?;
            Ok(())
        })
    }

    pub fn inventory_json(&self, id: &str) -> Result<Option<String>> {
        self.with(|c| {
            Ok(c.query_row(
                "SELECT inventory_json FROM devices WHERE id=?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?)
        })
    }

    pub fn set_thumbnail_ts(&self, id: &str, ts: i64) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE devices SET thumbnail_ts=?2 WHERE id=?1",
                params![id, ts],
            )?;
            Ok(())
        })
    }

    pub fn set_device_policy(&self, id: &str, policy: &Policy) -> Result<()> {
        let json = serde_json::to_string(policy)?;
        self.with(|c| {
            c.execute(
                "UPDATE devices SET policy_json=?2 WHERE id=?1",
                params![id, json],
            )?;
            Ok(())
        })
    }

    pub fn all_device_ids(&self) -> Result<Vec<String>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT id FROM devices")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
    }

    // ---- events -----------------------------------------------------------

    pub fn add_event(
        &self,
        device_id: &str,
        kind: &str,
        severity: &str,
        title: &str,
        detail: &str,
    ) -> Result<i64> {
        let now = chrono::Utc::now().timestamp();
        self.with(|c| {
            c.execute(
                "INSERT INTO events(ts,device_id,kind,severity,title,detail)
                 VALUES(?1,?2,?3,?4,?5,?6)",
                params![now, device_id, kind, severity, title, detail],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn events(&self, limit: i64, kind: Option<&str>) -> Result<Vec<protocol::EventRow>> {
        self.with(|c| {
            let sql = if kind.is_some() {
                "SELECT e.id,e.ts,e.device_id,COALESCE(d.name,e.device_id),e.kind,e.severity,e.title,e.detail
                 FROM events e LEFT JOIN devices d ON d.id=e.device_id
                 WHERE e.kind=?1 ORDER BY e.ts DESC LIMIT ?2"
            } else {
                "SELECT e.id,e.ts,e.device_id,COALESCE(d.name,e.device_id),e.kind,e.severity,e.title,e.detail
                 FROM events e LEFT JOIN devices d ON d.id=e.device_id
                 ORDER BY e.ts DESC LIMIT ?1"
            };
            let mut stmt = c.prepare(sql)?;
            let map = |r: &Row| {
                Ok(protocol::EventRow {
                    id: r.get(0)?,
                    ts: r.get(1)?,
                    device_id: r.get(2)?,
                    device_name: r.get(3)?,
                    kind: r.get(4)?,
                    severity: r.get(5)?,
                    title: r.get(6)?,
                    detail: r.get(7)?,
                })
            };
            let rows = if let Some(k) = kind {
                stmt.query_map(params![k, limit], map)?
            } else {
                stmt.query_map(params![limit], map)?
            };
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
    }

    pub fn set_event_status(&self, id: i64, status: &str) -> Result<bool> {
        self.with(|c| {
            let n = c.execute("UPDATE events SET status=?2 WHERE id=?1", params![id, status])?;
            Ok(n > 0)
        })
    }

    pub fn prune_events(&self, older_than_days: u64) -> Result<usize> {
        let cutoff = chrono::Utc::now().timestamp() - (older_than_days * 86400) as i64;
        self.with(|c| {
            let n = c.execute("DELETE FROM events WHERE ts < ?1", params![cutoff])?;
            Ok(n)
        })
    }

    // ---- commands ---------------------------------------------------------

    pub fn queue_command(
        &self,
        id: &str,
        device_id: &str,
        command_json: &str,
        actor: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.with(|c| {
            c.execute(
                "INSERT INTO commands(id,ts,device_id,command_json,actor)
                 VALUES(?1,?2,?3,?4,?5)",
                params![id, now, device_id, command_json, actor],
            )?;
            Ok(())
        })
    }

    pub fn mark_command(&self, id: &str, status: &str, result: Option<&str>) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE commands SET status=?2, result=?3 WHERE id=?1",
                params![id, status, result],
            )?;
            Ok(())
        })
    }

    pub fn take_queued_commands(&self, device_id: &str) -> Result<Vec<(String, String)>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, command_json FROM commands
                 WHERE device_id=?1 AND status='queued' ORDER BY ts LIMIT 50",
            )?;
            let rows = stmt.query_map(params![device_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
    }

    pub fn prune_commands(&self, older_than_days: u64) -> Result<usize> {
        let cutoff = chrono::Utc::now().timestamp() - (older_than_days * 86400) as i64;
        self.with(|c| {
            let n = c.execute("DELETE FROM commands WHERE ts < ?1", params![cutoff])?;
            Ok(n)
        })
    }

    // ---- audit ------------------------------------------------------------

    pub fn audit(&self, actor: &str, action: &str, target: &str, detail: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.with(|c| {
            c.execute(
                "INSERT INTO audit_log(ts,actor,action,target,detail) VALUES(?1,?2,?3,?4,?5)",
                params![now, actor, action, target, detail],
            )?;
            Ok(())
        })
    }

    pub fn audit_rows(&self, limit: i64) -> Result<Vec<protocol::AuditRow>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id,ts,actor,action,target,detail FROM audit_log ORDER BY ts DESC LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![limit], |r| {
                Ok(protocol::AuditRow {
                    id: r.get(0)?,
                    ts: r.get(1)?,
                    actor: r.get(2)?,
                    action: r.get(3)?,
                    target: r.get(4)?,
                    detail: r.get(5)?,
                })
            })?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
    }

    // ---- sessions ---------------------------------------------------------

    pub fn create_session(
        &self,
        token: &str,
        username: &str,
        role: &str,
        ttl_secs: u64,
    ) -> Result<()> {
        let expires = chrono::Utc::now().timestamp() + ttl_secs as i64;
        self.with(|c| {
            c.execute(
                "INSERT INTO sessions(token_hash,username,role,expires) VALUES(?1,?2,?3,?4)",
                params![hash(token), username, role, expires],
            )?;
            Ok(())
        })
    }

    pub fn session(&self, token: &str) -> Result<Option<(String, String)>> {
        let now = chrono::Utc::now().timestamp();
        self.with(|c| {
            let found: Option<(String, String, i64)> = c
                .query_row(
                    "SELECT username,role,expires FROM sessions WHERE token_hash=?1",
                    params![hash(token)],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            match found {
                Some((u, r, exp)) if exp > now => Ok(Some((u, r))),
                _ => Ok(None),
            }
        })
    }

    pub fn prune_sessions(&self) -> Result<usize> {
        let now = chrono::Utc::now().timestamp();
        self.with(|c| {
            let n = c.execute("DELETE FROM sessions WHERE expires < ?1", params![now])?;
            Ok(n)
        })
    }

    // ---- enrollment tokens -------------------------------------------------

    pub fn add_enrollment_token(&self, token: &str, label: &str, expires_at: Option<i64>) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.with(|c| {
            c.execute(
                "INSERT INTO enrollment_tokens(token,label,created_at,expires_at)
                 VALUES(?1,?2,?3,?4)",
                params![token, label, now, expires_at],
            )?;
            Ok(())
        })
    }

    pub fn valid_enrollment_token(&self, token: &str) -> Result<bool> {
        let now = chrono::Utc::now().timestamp();
        self.with(|c| {
            let found: Option<i64> = c
                .query_row(
                    "SELECT COALESCE(expires_at, 4102444800) FROM enrollment_tokens WHERE token=?1",
                    params![token],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(found.map(|exp| exp > now).unwrap_or(false))
        })
    }

    pub fn bump_enrollment_token(&self, token: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE enrollment_tokens SET used_count=used_count+1 WHERE token=?1",
                params![token],
            )?;
            Ok(())
        })
    }

    pub fn list_enrollment_tokens(&self) -> Result<Vec<(String, String, i64, i64, i64)>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT token,label,created_at,COALESCE(expires_at,0),used_count
                 FROM enrollment_tokens ORDER BY created_at DESC",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            })?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
    }

    pub fn delete_enrollment_token(&self, token: &str) -> Result<bool> {
        self.with(|c| {
            let n = c.execute("DELETE FROM enrollment_tokens WHERE token=?1", params![token])?;
            Ok(n > 0)
        })
    }

    // ---- keywords / policy template ---------------------------------------

    pub fn keywords(&self) -> Result<Vec<KeywordRule>> {
        match self.get_setting("keywords")? {
            Some(json) => Ok(serde_json::from_str(&json)
                .map_err(|e| anyhow!("bad keywords json: {e}"))?),
            None => Ok(Vec::new()),
        }
    }

    pub fn stats(&self, offline_after_secs: u64) -> Result<serde_json::Value> {
        let cutoff = chrono::Utc::now().timestamp() - offline_after_secs as i64;
        let day_ago = chrono::Utc::now().timestamp() - 86400;
        self.with(|c| {
            let total: i64 = c.query_row("SELECT COUNT(*) FROM devices", [], |r| r.get(0))?;
            let online: i64 = c.query_row(
                "SELECT COUNT(*) FROM devices WHERE last_seen >= ?1",
                params![cutoff],
                |r| r.get(0),
            )?;
            let alerts: i64 = c.query_row(
                "SELECT COUNT(*) FROM events WHERE kind='alert' AND ts >= ?1",
                params![day_ago],
                |r| r.get(0),
            )?;
            let locked: i64 = c.query_row(
                "SELECT COUNT(*) FROM devices WHERE json_extract(policy_json,'$.locked') = 1",
                [],
                |r| r.get(0),
            )?;
            let blanked: i64 = c.query_row(
                "SELECT COUNT(*) FROM devices WHERE json_extract(policy_json,'$.blanked') = 1",
                [],
                |r| r.get(0),
            )?;
            Ok(serde_json::json!({
                "devices_total": total,
                "devices_online": online,
                "alerts_24h": alerts,
                "devices_locked": locked,
                "devices_blanked": blanked,
                "server_time": chrono::Utc::now().timestamp(),
            }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_lifecycle() {
        let db = Db::open_in_memory().unwrap();
        let policy = Policy::default();
        db.create_device("d1", "Lab-1", "lab1", "macos", "15", "arm64", "0.1.0", "tok", &policy)
            .unwrap();
        assert!(db.device_token_valid("d1", "tok").unwrap());
        assert!(!db.device_token_valid("d1", "wrong").unwrap());
        db.update_heartbeat("d1", "alice", "10.0.0.5", 12.0, 4096, 16384, Some(90.0), false, 100)
            .unwrap();
        let devices = db.devices(20).unwrap();
        assert_eq!(devices.len(), 1);
        assert!(devices[0].online);
        assert_eq!(devices[0].active_user, "alice");
        assert_eq!(devices[0].battery_pct, Some(90.0));
    }

    #[test]
    fn sessions_expire() {
        let db = Db::open_in_memory().unwrap();
        db.create_session("t1", "admin", "admin", 60).unwrap();
        assert_eq!(db.session("t1").unwrap().unwrap().0, "admin");
        db.with(|c| {
            c.execute("UPDATE sessions SET expires=1", []).unwrap();
            Ok(())
        })
        .unwrap();
        assert!(db.session("t1").unwrap().is_none());
    }

    #[test]
    fn enrollment_tokens() {
        let db = Db::open_in_memory().unwrap();
        db.add_enrollment_token("enroll-me", "lab", None).unwrap();
        assert!(db.valid_enrollment_token("enroll-me").unwrap());
        assert!(!db.valid_enrollment_token("nope").unwrap());
        db.bump_enrollment_token("enroll-me").unwrap();
        assert_eq!(db.list_enrollment_tokens().unwrap()[0].4, 1);
    }

    #[test]
    fn command_queue() {
        let db = Db::open_in_memory().unwrap();
        db.queue_command("c1", "d1", "{\"type\":\"nop\"}", "admin").unwrap();
        let queued = db.take_queued_commands("d1").unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].0, "c1");
    }

    #[test]
    fn events_both_query_shapes() {
        let db = Db::open_in_memory().unwrap();
        db.create_device("d1", "Lab-1", "lab1", "linux", "24.04", "x86_64", "0.1.0", "t", &Policy::default())
            .unwrap();
        db.add_event("d1", "alert", "high", "kw", "detail").unwrap();
        db.add_event("d1", "activity", "info", "app", "{}").unwrap();
        // Unfiltered branch (single bind) must work.
        assert_eq!(db.events(50, None).unwrap().len(), 2);
        // Filtered branch must work and join device names.
        let alerts = db.events(50, Some("alert")).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].device_name, "Lab-1");
        assert_eq!(db.events(50, Some("missing")).unwrap().len(), 0);
    }
}
