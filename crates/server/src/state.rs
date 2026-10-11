//! Shared server state: connected device channels + console broadcast.

use protocol::ServerMessage;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

pub type DeviceTx = mpsc::Sender<ServerMessage>;

#[derive(Default)]
pub struct Inner {
    /// device_id -> outbound channel
    pub devices: HashMap<String, DeviceTx>,
}

pub struct AppState {
    pub db: crate::db::Db,
    pub config: crate::config::Config,
    pub inner: RwLock<Inner>,
    /// Set when a console is actively viewing thumbnails (drives agent FPS).
    pub viewing: AtomicBool,
    pub last_view: AtomicI64,
    /// Broadcast channel to console websockets.
    pub console_tx: tokio::sync::broadcast::Sender<String>,
}

pub type Shared = Arc<AppState>;

impl AppState {
    pub fn new(db: crate::db::Db, config: crate::config::Config) -> Shared {
        let (console_tx, _) = tokio::sync::broadcast::channel(256);
        Arc::new(AppState {
            db,
            config,
            inner: RwLock::new(Inner::default()),
            viewing: AtomicBool::new(false),
            last_view: AtomicI64::new(0),
            console_tx,
        })
    }

    pub async fn register_device(&self, id: &str, tx: DeviceTx) {
        self.inner.write().await.devices.insert(id.to_string(), tx);
    }

    pub async fn unregister_device(&self, id: &str) {
        self.inner.write().await.devices.remove(id);
    }

    pub async fn device_online(&self, id: &str) -> bool {
        self.inner.read().await.devices.contains_key(id)
    }

    /// Send a message to one device; false when offline.
    pub async fn send_device(&self, id: &str, msg: ServerMessage) -> bool {
        let tx = self.inner.read().await.devices.get(id).cloned();
        match tx {
            Some(tx) => tx.send(msg).await.is_ok(),
            None => false,
        }
    }

    /// Broadcast to every connected device; returns delivery count.
    pub async fn broadcast_devices(&self, msg: ServerMessage) -> usize {
        let senders: Vec<DeviceTx> = {
            let inner = self.inner.read().await;
            inner.devices.values().cloned().collect()
        };
        let mut n = 0;
        for tx in senders {
            if tx.send(msg.clone()).await.is_ok() {
                n += 1;
            }
        }
        n
    }

    /// Push a console event (JSON) to every connected console websocket.
    pub fn notify_console(&self, kind: &str, payload: serde_json::Value) {
        let msg = serde_json::json!({ "type": kind, "payload": payload });
        let _ = self.console_tx.send(msg.to_string());
    }

    pub fn mark_viewing(&self) {
        self.last_view
            .store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
    }
}
