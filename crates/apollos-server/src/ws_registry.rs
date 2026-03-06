use std::collections::HashMap;
use std::sync::Arc;

use apollos_proto::contracts::BackendToClientMessage;
use chrono::Utc;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct ManagedSocket {
    connection_id: String,
    client_id: Option<String>,
    connected_epoch_ms: i64,
    tx: mpsc::Sender<BackendToClientMessage>,
}

#[derive(Debug, Default)]
struct RegistryState {
    live: HashMap<String, ManagedSocket>,
    emergency: HashMap<String, ManagedSocket>,
    help_watchers: HashMap<String, HashMap<String, ManagedSocket>>,
}

#[derive(Debug, Clone, Default)]
pub struct WebSocketRegistry {
    inner: Arc<Mutex<RegistryState>>,
}

impl WebSocketRegistry {
    pub async fn register_live(
        &self,
        session_id: &str,
        tx: mpsc::Sender<BackendToClientMessage>,
        client_id: Option<String>,
    ) -> Result<String, String> {
        let mut guard = self.inner.lock().await;

        if single_instance_only() {
            if let Some((active_session_id, _)) = guard
                .live
                .iter()
                .find(|(active_session_id, _)| *active_session_id != session_id)
            {
                return Err(format!(
                    "single_instance_only: active live session exists ({active_session_id})"
                ));
            }
        }

        if let Some(active) = guard.live.get(session_id) {
            if active.client_id.is_some() && active.client_id != client_id {
                return Err("live session already owned by another client".to_string());
            }
        }

        let connection_id = Uuid::new_v4().to_string();
        guard.live.insert(
            session_id.to_string(),
            ManagedSocket {
                connection_id: connection_id.clone(),
                client_id,
                connected_epoch_ms: Utc::now().timestamp_millis(),
                tx,
            },
        );

        Ok(connection_id)
    }

    pub async fn register_emergency(
        &self,
        session_id: &str,
        tx: mpsc::Sender<BackendToClientMessage>,
        client_id: Option<String>,
    ) -> Result<String, String> {
        let mut guard = self.inner.lock().await;

        if single_instance_only() {
            if let Some((active_session_id, _)) = guard
                .emergency
                .iter()
                .find(|(active_session_id, _)| *active_session_id != session_id)
            {
                return Err(format!(
                    "single_instance_only: active emergency session exists ({active_session_id})"
                ));
            }
        }

        if let Some(live) = guard.live.get(session_id) {
            if live.client_id.is_some() && live.client_id != client_id {
                return Err("emergency channel client mismatch".to_string());
            }
        }

        if let Some(active) = guard.emergency.get(session_id) {
            if active.client_id.is_some() && active.client_id != client_id {
                return Err("emergency channel already owned by another client".to_string());
            }
        }

        let connection_id = Uuid::new_v4().to_string();
        guard.emergency.insert(
            session_id.to_string(),
            ManagedSocket {
                connection_id: connection_id.clone(),
                client_id,
                connected_epoch_ms: Utc::now().timestamp_millis(),
                tx,
            },
        );

        Ok(connection_id)
    }

    pub async fn unregister_live(&self, session_id: &str, connection_id: Option<&str>) -> bool {
        let mut guard = self.inner.lock().await;
        let Some(active) = guard.live.get(session_id) else {
            return false;
        };

        if let Some(expected) = connection_id {
            if active.connection_id != expected {
                return false;
            }
        }

        guard.live.remove(session_id);
        true
    }

    pub async fn unregister_emergency(
        &self,
        session_id: &str,
        connection_id: Option<&str>,
    ) -> bool {
        let mut guard = self.inner.lock().await;
        let Some(active) = guard.emergency.get(session_id) else {
            return false;
        };

        if let Some(expected) = connection_id {
            if active.connection_id != expected {
                return false;
            }
        }

        guard.emergency.remove(session_id);
        true
    }

    pub async fn register_help_viewer(
        &self,
        session_id: &str,
        viewer_id: &str,
        tx: mpsc::Sender<BackendToClientMessage>,
    ) {
        let mut guard = self.inner.lock().await;
        let bucket = guard
            .help_watchers
            .entry(session_id.to_string())
            .or_default();

        bucket.insert(
            viewer_id.to_string(),
            ManagedSocket {
                connection_id: Uuid::new_v4().to_string(),
                client_id: Some(viewer_id.to_string()),
                connected_epoch_ms: Utc::now().timestamp_millis(),
                tx,
            },
        );
    }

    pub async fn unregister_help_viewer(&self, session_id: &str, viewer_id: &str) {
        let mut guard = self.inner.lock().await;
        let Some(bucket) = guard.help_watchers.get_mut(session_id) else {
            return;
        };

        bucket.remove(viewer_id);
        if bucket.is_empty() {
            guard.help_watchers.remove(session_id);
        }
    }

    pub async fn send_live(&self, session_id: &str, payload: BackendToClientMessage) -> bool {
        let target = {
            let guard = self.inner.lock().await;
            guard.live.get(session_id).cloned()
        };

        let Some(target) = target else {
            return false;
        };

        if target.tx.send(payload).await.is_err() {
            let _ = self
                .unregister_live(session_id, Some(target.connection_id.as_str()))
                .await;
            return false;
        }

        true
    }

    pub async fn send_emergency(&self, session_id: &str, payload: BackendToClientMessage) -> bool {
        let target = {
            let guard = self.inner.lock().await;
            guard.emergency.get(session_id).cloned()
        };

        let Some(target) = target else {
            return false;
        };

        if target.tx.send(payload).await.is_err() {
            let _ = self
                .unregister_emergency(session_id, Some(target.connection_id.as_str()))
                .await;
            return false;
        }

        true
    }

    pub async fn emit_hard_stop(&self, session_id: &str, payload: BackendToClientMessage) {
        if self.send_emergency(session_id, payload.clone()).await {
            return;
        }

        let _ = self.send_live(session_id, payload).await;
    }

    pub async fn send_help(&self, session_id: &str, payload: BackendToClientMessage) -> usize {
        let targets = {
            let guard = self.inner.lock().await;
            guard
                .help_watchers
                .get(session_id)
                .map(|bucket| {
                    bucket
                        .iter()
                        .map(|(viewer_id, managed)| (viewer_id.clone(), managed.clone()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };

        let mut delivered = 0;
        for (viewer_id, managed) in targets {
            if managed.tx.send(payload.clone()).await.is_ok() {
                delivered += 1;
            } else {
                self.unregister_help_viewer(session_id, &viewer_id).await;
            }
        }

        delivered
    }

    pub async fn live_connection_age_ms(&self, session_id: &str) -> Option<i64> {
        let guard = self.inner.lock().await;
        let managed = guard.live.get(session_id)?;
        Some(Utc::now().timestamp_millis() - managed.connected_epoch_ms)
    }
}

fn single_instance_only() -> bool {
    let value = std::env::var("SINGLE_INSTANCE_ONLY")
        .unwrap_or_else(|_| "1".to_string())
        .to_ascii_lowercase();

    !matches!(value.trim(), "0" | "false" | "off" | "no")
}

#[cfg(test)]
mod tests {
    use apollos_proto::contracts::{
        BackendToClientMessage, ConnectionState, ConnectionStateMessage,
    };

    use super::*;

    fn connection_state_payload() -> BackendToClientMessage {
        BackendToClientMessage::ConnectionState(ConnectionStateMessage {
            state: ConnectionState::Connected,
            detail: Some("test".to_string()),
        })
    }

    #[tokio::test]
    async fn emergency_registration_requires_same_owner_as_live() {
        let registry = WebSocketRegistry::default();

        let (live_tx, _live_rx) = mpsc::channel(8);
        let (emergency_tx, _emergency_rx) = mpsc::channel(8);

        registry
            .register_live("s1", live_tx, Some("client-a".to_string()))
            .await
            .expect("live should register");

        let result = registry
            .register_emergency("s1", emergency_tx, Some("client-b".to_string()))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn hard_stop_falls_back_to_live_when_emergency_missing() {
        let registry = WebSocketRegistry::default();
        let (tx, mut rx) = mpsc::channel(8);

        registry
            .register_live("s2", tx, Some("client-a".to_string()))
            .await
            .expect("live should register");

        registry
            .emit_hard_stop("s2", connection_state_payload())
            .await;

        let received = rx.recv().await;
        assert!(received.is_some());
    }

    #[tokio::test]
    async fn help_watchers_receive_broadcast() {
        let registry = WebSocketRegistry::default();
        let (tx1, mut rx1) = mpsc::channel(8);
        let (tx2, mut rx2) = mpsc::channel(8);

        registry.register_help_viewer("s3", "v1", tx1).await;
        registry.register_help_viewer("s3", "v2", tx2).await;

        let delivered = registry.send_help("s3", connection_state_payload()).await;
        assert_eq!(delivered, 2);
        assert!(rx1.recv().await.is_some());
        assert!(rx2.recv().await.is_some());
    }

    #[tokio::test]
    async fn single_instance_only_rejects_second_live_session() {
        let registry = WebSocketRegistry::default();
        let (tx1, _rx1) = mpsc::channel(8);
        let (tx2, _rx2) = mpsc::channel(8);

        registry
            .register_live("s-a", tx1, Some("client-a".to_string()))
            .await
            .expect("first session should register");

        let second = registry
            .register_live("s-b", tx2, Some("client-b".to_string()))
            .await;

        assert!(second.is_err());
    }
}
