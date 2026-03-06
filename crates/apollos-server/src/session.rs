use std::{collections::HashMap, sync::Arc};

use anyhow::Context;
use apollos_proto::contracts::{MotionState, NavigationMode};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tokio::sync::RwLock;
use tracing::warn;

const GEOHASH_BASE32: &str = "0123456789bcdefghjkmnpqrstuvwxyz";

#[derive(Debug, Clone)]
pub struct SessionState {
    pub session_id: String,
    pub mode: NavigationMode,
    pub context_summary: String,
    pub motion_state: MotionState,
    pub last_seen: DateTime<Utc>,
    pub mode_override_until_epoch: f64,
    pub mode_override_reason: String,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub heading_deg: Option<f32>,
    pub frame_sequence: u64,
    pub edge_hazard_until_epoch: f64,
    pub edge_hazard_type: String,
    pub sensor_health_score: f32,
    pub sensor_health_flags: Vec<String>,
    pub localization_uncertainty_m: f32,
    pub degraded_mode: bool,
    pub degraded_reason: String,
    pub last_hazard_score: f32,
    pub last_hard_stop: bool,
    pub utterance_timestamps: Vec<f64>,
    pub last_persist_epoch: f64,
}

impl SessionState {
    fn new(session_id: String) -> Self {
        Self {
            session_id,
            mode: NavigationMode::Navigation,
            context_summary: String::new(),
            motion_state: MotionState::Stationary,
            last_seen: Utc::now(),
            mode_override_until_epoch: 0.0,
            mode_override_reason: String::new(),
            lat: None,
            lng: None,
            heading_deg: None,
            frame_sequence: 0,
            edge_hazard_until_epoch: 0.0,
            edge_hazard_type: String::new(),
            sensor_health_score: 1.0,
            sensor_health_flags: Vec::new(),
            localization_uncertainty_m: 120.0,
            degraded_mode: false,
            degraded_reason: String::new(),
            last_hazard_score: 0.0,
            last_hard_stop: false,
            utterance_timestamps: Vec::new(),
            last_persist_epoch: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionObservability {
    pub motion_state: MotionState,
    pub sensor_health_score: f32,
    pub sensor_health_flags: Vec<String>,
    pub localization_uncertainty_m: f32,
    pub degraded_mode: bool,
    pub degraded_reason: String,
    pub edge_reflex_active: bool,
    pub edge_hazard_type: String,
    pub last_hazard_score: f32,
    pub last_hard_stop: bool,
}

#[derive(Debug, Clone)]
pub struct SessionLocationSnapshot {
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub heading_deg: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrowdHazardRecord {
    geohash: String,
    geohash_prefix5: String,
    hazard_type: String,
    confirmed_count: u64,
    last_confirmed: String,
    description_vi: String,
    lat: f64,
    lng: f64,
    heading_deg: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct SessionStore {
    inner: Arc<RwLock<HashMap<String, SessionState>>>,
    hazards: Arc<RwLock<HashMap<String, Vec<Value>>>>,
    emotions: Arc<RwLock<HashMap<String, Vec<Value>>>>,
    crowd_hazard_map: Arc<RwLock<HashMap<String, CrowdHazardRecord>>>,
    persist_min_interval_s: f64,
    persistence: Option<Arc<FirestorePersistence>>,
}

impl Default for SessionStore {
    fn default() -> Self {
        let persist_min_interval_s = std::env::var("SESSION_PERSIST_MIN_INTERVAL_S")
            .ok()
            .and_then(|raw| raw.parse::<f64>().ok())
            .unwrap_or(1.5)
            .max(0.2);

        let persistence = FirestorePersistence::from_env();

        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            hazards: Arc::new(RwLock::new(HashMap::new())),
            emotions: Arc::new(RwLock::new(HashMap::new())),
            crowd_hazard_map: Arc::new(RwLock::new(HashMap::new())),
            persist_min_interval_s,
            persistence,
        }
    }
}

impl SessionStore {
    const SENSOR_HEALTH_DEGRADED_THRESHOLD: f32 = 0.55;
    const LOCALIZATION_UNCERTAINTY_DEGRADED_M: f32 = 6.0;

    pub async fn ensure_session(&self, session_id: &str) -> SessionState {
        let mut guard = self.inner.write().await;
        guard
            .entry(session_id.to_string())
            .or_insert_with(|| SessionState::new(session_id.to_string()))
            .clone()
    }

    pub async fn touch_session(
        &self,
        session_id: &str,
        motion_state: Option<MotionState>,
        lat: Option<f64>,
        lng: Option<f64>,
        heading_deg: Option<f32>,
        advance_frame: bool,
    ) -> SessionState {
        let (result, should_persist) = {
            let mut guard = self.inner.write().await;
            let state = guard
                .entry(session_id.to_string())
                .or_insert_with(|| SessionState::new(session_id.to_string()));

            if let Some(motion_state) = motion_state {
                state.motion_state = motion_state;
            }

            if let Some(lat) = lat {
                state.lat = Some(lat);
            }
            if let Some(lng) = lng {
                state.lng = Some(lng);
            }
            if let Some(heading_deg) = heading_deg {
                state.heading_deg = Some(heading_deg);
            }

            if advance_frame {
                state.frame_sequence = state.frame_sequence.saturating_add(1);
            }

            state.last_seen = Utc::now();
            let should_persist = self.mark_persist_if_due(state, false);
            (state.clone(), should_persist)
        };

        if should_persist {
            self.persist_session_async(result.clone());
        }

        result
    }

    pub async fn set_mode(&self, session_id: &str, mode: NavigationMode) -> SessionState {
        let (result, should_persist) = {
            let mut guard = self.inner.write().await;
            let state = guard
                .entry(session_id.to_string())
                .or_insert_with(|| SessionState::new(session_id.to_string()));

            state.mode = mode;
            state.mode_override_until_epoch = 0.0;
            state.mode_override_reason.clear();
            state.last_seen = Utc::now();

            let should_persist = self.mark_persist_if_due(state, true);
            (state.clone(), should_persist)
        };

        if should_persist {
            self.persist_session_async(result.clone());
        }

        result
    }

    pub async fn get_effective_mode(&self, session_id: &str) -> NavigationMode {
        self.get_effective_mode_at(session_id, now_epoch()).await
    }

    async fn get_effective_mode_at(&self, session_id: &str, now: f64) -> NavigationMode {
        let (mode, persist_snapshot) = {
            let mut guard = self.inner.write().await;
            let state = guard
                .entry(session_id.to_string())
                .or_insert_with(|| SessionState::new(session_id.to_string()));

            if state.mode_override_until_epoch > now {
                return NavigationMode::Navigation;
            }

            let mut snapshot = None;
            if state.mode_override_until_epoch > 0.0 {
                state.mode_override_until_epoch = 0.0;
                state.mode_override_reason.clear();
                if self.mark_persist_if_due(state, true) {
                    snapshot = Some(state.clone());
                }
            }

            (state.mode, snapshot)
        };

        if let Some(snapshot) = persist_snapshot {
            self.persist_session_async(snapshot);
        }

        mode
    }

    pub async fn apply_stress_mode_override(
        &self,
        session_id: &str,
        reason: &str,
        revert_after_seconds: u64,
    ) -> SessionState {
        let (result, should_persist) = {
            let mut guard = self.inner.write().await;
            let state = guard
                .entry(session_id.to_string())
                .or_insert_with(|| SessionState::new(session_id.to_string()));

            let duration = revert_after_seconds.max(30) as f64;
            state.mode_override_until_epoch = now_epoch() + duration;
            state.mode_override_reason = reason.to_string();
            state.last_seen = Utc::now();

            let should_persist = self.mark_persist_if_due(state, true);
            (state.clone(), should_persist)
        };

        if should_persist {
            self.persist_session_async(result.clone());
        }

        result
    }

    pub async fn update_context_summary(&self, session_id: &str, summary: String) -> SessionState {
        let (result, should_persist) = {
            let mut guard = self.inner.write().await;
            let state = guard
                .entry(session_id.to_string())
                .or_insert_with(|| SessionState::new(session_id.to_string()));

            state.context_summary = summary;
            state.last_seen = Utc::now();

            let should_persist = self.mark_persist_if_due(state, true);
            (state.clone(), should_persist)
        };

        if should_persist {
            self.persist_session_async(result.clone());
        }

        result
    }

    pub async fn get_context_summary(&self, session_id: &str) -> String {
        let guard = self.inner.read().await;
        let Some(state) = guard.get(session_id) else {
            return "No context available".to_string();
        };

        if !state.context_summary.trim().is_empty() {
            return state.context_summary.clone();
        }

        let edge_state = if state.edge_hazard_until_epoch > now_epoch() {
            format!("; edge_reflex={}", state.edge_hazard_type)
        } else {
            String::new()
        };

        let degraded_state = if state.degraded_mode {
            format!("; degraded={}", state.degraded_reason)
        } else {
            String::new()
        };

        format!(
            "mode={:?}; motion={:?}; sensor_health={:.2}; loc_uncertainty_m={:.1}{edge_state}{degraded_state}",
            state.mode, state.motion_state, state.sensor_health_score, state.localization_uncertainty_m
        )
    }

    pub async fn update_observability(
        &self,
        session_id: &str,
        sensor_health_score: Option<f32>,
        sensor_health_flags: Option<Vec<String>>,
        localization_uncertainty_m: Option<f32>,
        latest_hazard_score: Option<f32>,
        latest_hard_stop: Option<bool>,
        degraded_reason: Option<String>,
    ) -> SessionState {
        let (result, should_persist) = {
            let mut guard = self.inner.write().await;
            let state = guard
                .entry(session_id.to_string())
                .or_insert_with(|| SessionState::new(session_id.to_string()));

            if let Some(score) = sensor_health_score {
                state.sensor_health_score = score.clamp(0.0, 1.0);
            }

            if let Some(flags) = sensor_health_flags {
                state.sensor_health_flags = flags;
            }

            if let Some(uncertainty) = localization_uncertainty_m {
                state.localization_uncertainty_m = uncertainty.max(0.0);
            }

            if let Some(hazard_score) = latest_hazard_score {
                state.last_hazard_score = hazard_score.max(0.0);
            }

            if let Some(hard_stop) = latest_hard_stop {
                state.last_hard_stop = hard_stop;
            }

            if let Some(reason) = degraded_reason {
                state.degraded_reason = reason;
            } else if state.sensor_health_score < Self::SENSOR_HEALTH_DEGRADED_THRESHOLD {
                state.degraded_reason = "sensor_health_low".to_string();
            } else if state.localization_uncertainty_m > Self::LOCALIZATION_UNCERTAINTY_DEGRADED_M {
                state.degraded_reason = "localization_uncertain".to_string();
            } else {
                state.degraded_reason.clear();
            }

            state.degraded_mode = !state.degraded_reason.is_empty();
            state.last_seen = Utc::now();

            let should_persist = self.mark_persist_if_due(state, true);
            (state.clone(), should_persist)
        };

        if should_persist {
            self.persist_session_async(result.clone());
        }

        result
    }

    pub async fn get_observability(&self, session_id: &str) -> SessionObservability {
        let guard = self.inner.read().await;
        let Some(state) = guard.get(session_id) else {
            return SessionObservability {
                motion_state: MotionState::Stationary,
                sensor_health_score: 1.0,
                sensor_health_flags: Vec::new(),
                localization_uncertainty_m: 120.0,
                degraded_mode: false,
                degraded_reason: String::new(),
                edge_reflex_active: false,
                edge_hazard_type: String::new(),
                last_hazard_score: 0.0,
                last_hard_stop: false,
            };
        };

        SessionObservability {
            motion_state: state.motion_state,
            sensor_health_score: state.sensor_health_score,
            sensor_health_flags: state.sensor_health_flags.clone(),
            localization_uncertainty_m: state.localization_uncertainty_m,
            degraded_mode: state.degraded_mode,
            degraded_reason: state.degraded_reason.clone(),
            edge_reflex_active: state.edge_hazard_until_epoch > now_epoch(),
            edge_hazard_type: state.edge_hazard_type.clone(),
            last_hazard_score: state.last_hazard_score,
            last_hard_stop: state.last_hard_stop,
        }
    }

    pub async fn mark_edge_hazard(
        &self,
        session_id: &str,
        hazard_type: String,
        suppress_seconds: Option<u32>,
    ) -> SessionState {
        let (result, should_persist) = {
            let mut guard = self.inner.write().await;
            let state = guard
                .entry(session_id.to_string())
                .or_insert_with(|| SessionState::new(session_id.to_string()));

            let suppress = suppress_seconds.unwrap_or(3).max(1) as f64;
            state.edge_hazard_until_epoch = now_epoch() + suppress;
            state.edge_hazard_type = hazard_type;
            state.last_seen = Utc::now();

            let should_persist = self.mark_persist_if_due(state, true);
            (state.clone(), should_persist)
        };

        if should_persist {
            self.persist_session_async(result.clone());
        }

        result
    }

    pub async fn is_edge_hazard_active(&self, session_id: &str) -> bool {
        self.is_edge_hazard_active_at(session_id, now_epoch()).await
    }

    async fn is_edge_hazard_active_at(&self, session_id: &str, now: f64) -> bool {
        let guard = self.inner.read().await;
        guard
            .get(session_id)
            .map(|state| state.edge_hazard_until_epoch > now)
            .unwrap_or(false)
    }

    pub async fn location_snapshot(&self, session_id: &str) -> SessionLocationSnapshot {
        let guard = self.inner.read().await;
        let Some(state) = guard.get(session_id) else {
            return SessionLocationSnapshot {
                lat: None,
                lng: None,
                heading_deg: None,
            };
        };

        SessionLocationSnapshot {
            lat: state.lat,
            lng: state.lng,
            heading_deg: state.heading_deg,
        }
    }

    pub async fn should_allow_utterance(
        &self,
        session_id: &str,
        now_epoch: f64,
        min_gap_seconds: f64,
        burst_limit: usize,
        burst_window_seconds: f64,
    ) -> bool {
        let mut guard = self.inner.write().await;
        let state = guard
            .entry(session_id.to_string())
            .or_insert_with(|| SessionState::new(session_id.to_string()));

        state
            .utterance_timestamps
            .retain(|ts| now_epoch - *ts <= burst_window_seconds.max(1.0));

        if let Some(last) = state.utterance_timestamps.last().copied() {
            if (now_epoch - last) < min_gap_seconds.max(0.05) {
                return false;
            }
        }

        if state.utterance_timestamps.len() >= burst_limit.max(1) {
            return false;
        }

        state.utterance_timestamps.push(now_epoch);
        true
    }

    pub async fn log_hazard(
        &self,
        session_id: &str,
        hazard_type: &str,
        bearing_x: f32,
        distance_m: Option<f32>,
        relative_velocity_mps: Option<f32>,
        confidence: f32,
        hazard_score: Option<f32>,
        hard_stop: Option<bool>,
        source: Option<&str>,
        description: &str,
    ) {
        let timestamp = Utc::now().to_rfc3339();
        let distance_m = distance_m.map(|value| value.max(0.0));
        let distance_band = distance_m.map(distance_band_for_m);
        let event = json!({
            "hazard_type": hazard_type,
            "bearing_x": bearing_x,
            "distance_m": distance_m,
            "distance_band": distance_band,
            "relative_velocity_mps": relative_velocity_mps,
            "confidence": confidence,
            "hazard_score": hazard_score,
            "hard_stop": hard_stop,
            "source": source.unwrap_or("unknown"),
            "description": description,
            "ts": timestamp,
        });

        {
            let mut guard = self.hazards.write().await;
            let bucket = guard.entry(session_id.to_string()).or_default();
            bucket.push(event.clone());
            if bucket.len() > 50 {
                bucket.remove(0);
            }
        }

        let mut crowd_seed: Option<(String, CrowdHazardRecord)> = None;
        {
            let guard = self.inner.read().await;
            if let Some(state) = guard.get(session_id) {
                if let (Some(lat), Some(lng)) = (state.lat, state.lng) {
                    let geohash = encode_geohash(lat, lng, 7);
                    let doc_id = format!("{}-{}", geohash, sanitize_doc_id_fragment(hazard_type));

                    let mut crowd_guard = self.crowd_hazard_map.write().await;
                    let entry =
                        crowd_guard
                            .entry(doc_id.clone())
                            .or_insert_with(|| CrowdHazardRecord {
                                geohash: geohash.clone(),
                                geohash_prefix5: geohash.chars().take(5).collect(),
                                hazard_type: hazard_type.to_string(),
                                confirmed_count: 0,
                                last_confirmed: timestamp.clone(),
                                description_vi: description.to_string(),
                                lat,
                                lng,
                                heading_deg: state.heading_deg,
                            });

                    entry.confirmed_count = entry.confirmed_count.saturating_add(1);
                    entry.last_confirmed = timestamp.clone();
                    entry.description_vi = if description.trim().is_empty() {
                        hazard_type.to_string()
                    } else {
                        description.to_string()
                    };
                    entry.heading_deg = state.heading_deg;
                    crowd_seed = Some((doc_id, entry.clone()));
                }
            }
        }

        self.persist_subcollection_async(session_id.to_string(), "hazards", event);

        if let Some((doc_id, record)) = crowd_seed {
            let payload = serde_json::to_value(record).unwrap_or_else(|_| json!({}));
            self.persist_hazard_map_async(doc_id, payload);
        }
    }

    pub async fn log_emotion(&self, session_id: &str, emotion_state: &str, confidence: f32) {
        let event = json!({
            "state": emotion_state,
            "confidence": confidence.clamp(0.0, 1.0),
            "ts": Utc::now().to_rfc3339(),
        });

        {
            let mut guard = self.emotions.write().await;
            let bucket = guard.entry(session_id.to_string()).or_default();
            bucket.push(event.clone());
            if bucket.len() > 50 {
                bucket.remove(0);
            }
        }

        self.persist_subcollection_async(session_id.to_string(), "emotions", event);
    }

    pub async fn get_crowd_hazard_hints(&self, lat: f64, lng: f64, limit: usize) -> Vec<String> {
        let prefix5: String = encode_geohash(lat, lng, 5);
        let mut records = {
            let guard = self.crowd_hazard_map.read().await;
            guard
                .values()
                .filter(|record| record.geohash_prefix5 == prefix5)
                .cloned()
                .collect::<Vec<_>>()
        };

        records.sort_by_key(|record| std::cmp::Reverse(record.confirmed_count));

        records
            .into_iter()
            .take(limit.max(1))
            .map(|record| {
                format!(
                    "{} ({}) - {} confirmations",
                    record.hazard_type, record.description_vi, record.confirmed_count
                )
            })
            .collect()
    }

    pub async fn build_human_help_link(&self, session_id: &str) -> String {
        let public_help_base = std::env::var("PUBLIC_HELP_BASE")
            .unwrap_or_else(|_| "https://help.apollos.local/live".to_string());
        format!(
            "{}?session={session_id}",
            public_help_base.trim_end_matches('/')
        )
    }

    fn mark_persist_if_due(&self, state: &mut SessionState, force: bool) -> bool {
        if self.persistence.is_none() {
            return false;
        }

        let now = now_epoch();
        if !force && (now - state.last_persist_epoch) < self.persist_min_interval_s {
            return false;
        }

        state.last_persist_epoch = now;
        true
    }

    fn persist_session_async(&self, state: SessionState) {
        let Some(persistence) = self.persistence.clone() else {
            return;
        };

        tokio::spawn(async move {
            if let Err(error) = persistence.upsert_session(&state).await {
                warn!(
                    session_id = %state.session_id,
                    error = %error,
                    "failed to persist session to Firestore"
                );
            }
        });
    }

    fn persist_subcollection_async(
        &self,
        session_id: String,
        collection: &'static str,
        payload: Value,
    ) {
        let Some(persistence) = self.persistence.clone() else {
            return;
        };

        tokio::spawn(async move {
            if let Err(error) = persistence
                .append_subcollection(&session_id, collection, &payload)
                .await
            {
                warn!(
                    session_id = %session_id,
                    collection,
                    error = %error,
                    "failed to persist Firestore subcollection event"
                );
            }
        });
    }

    fn persist_hazard_map_async(&self, doc_id: String, payload: Value) {
        let Some(persistence) = self.persistence.clone() else {
            return;
        };

        tokio::spawn(async move {
            if let Err(error) = persistence.upsert_hazard_map(&doc_id, &payload).await {
                warn!(
                    doc_id = %doc_id,
                    error = %error,
                    "failed to persist Firestore hazard_map"
                );
            }
        });
    }
}

#[derive(Debug, Clone)]
struct FirestorePersistence {
    client: reqwest::Client,
    base_documents_url: String,
    static_bearer_token: Option<String>,
    metadata_token_cache: Arc<RwLock<Option<MetadataTokenCache>>>,
}

#[derive(Debug, Clone)]
struct MetadataTokenCache {
    access_token: String,
    expires_epoch: f64,
}

#[derive(Debug, Deserialize)]
struct MetadataTokenResponse {
    access_token: String,
    expires_in: u64,
}

impl FirestorePersistence {
    fn from_env() -> Option<Arc<Self>> {
        if !env_flag("USE_FIRESTORE", false) {
            return None;
        }

        let app_env = std::env::var("APP_ENV").unwrap_or_else(|_| "development".to_string());
        let firestore_required = env_flag(
            "FIRESTORE_REQUIRED",
            app_env.eq_ignore_ascii_case("production"),
        );

        let Some(project_id) = std::env::var("GOOGLE_CLOUD_PROJECT")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        else {
            if firestore_required {
                panic!("FIRESTORE_REQUIRED is enabled but GOOGLE_CLOUD_PROJECT is missing");
            } else {
                warn!("USE_FIRESTORE=1 but GOOGLE_CLOUD_PROJECT is missing; disabling Firestore persistence");
            }
            return None;
        };

        let base_documents_url = format!(
            "https://firestore.googleapis.com/v1/projects/{project_id}/databases/(default)/documents"
        );

        let static_bearer_token = std::env::var("FIRESTORE_BEARER_TOKEN")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        Some(Arc::new(Self {
            client: reqwest::Client::new(),
            base_documents_url,
            static_bearer_token,
            metadata_token_cache: Arc::new(RwLock::new(None)),
        }))
    }

    async fn upsert_session(&self, state: &SessionState) -> anyhow::Result<()> {
        let payload = json!({
            "mode": navigation_mode_str(state.mode),
            "context_summary": state.context_summary,
            "motion_state": motion_state_str(state.motion_state),
            "last_seen": state.last_seen.to_rfc3339(),
            "mode_override_until_epoch": state.mode_override_until_epoch,
            "mode_override_reason": state.mode_override_reason,
            "lat": state.lat,
            "lng": state.lng,
            "heading_deg": state.heading_deg,
            "frame_sequence": state.frame_sequence,
            "edge_hazard_until_epoch": state.edge_hazard_until_epoch,
            "edge_hazard_type": state.edge_hazard_type,
            "sensor_health_score": state.sensor_health_score,
            "sensor_health_flags": state.sensor_health_flags,
            "localization_uncertainty_m": state.localization_uncertainty_m,
            "degraded_mode": state.degraded_mode,
            "degraded_reason": state.degraded_reason,
            "last_hazard_score": state.last_hazard_score,
            "last_hard_stop": state.last_hard_stop,
        });

        let doc_path = format!("sessions/{}", sanitize_document_id(&state.session_id));
        self.upsert_document(&doc_path, &payload).await
    }

    async fn append_subcollection(
        &self,
        session_id: &str,
        collection: &str,
        payload: &Value,
    ) -> anyhow::Result<()> {
        let path = format!(
            "sessions/{}/{}",
            sanitize_document_id(session_id),
            sanitize_document_id(collection)
        );
        self.create_document(&path, payload).await
    }

    async fn upsert_hazard_map(&self, doc_id: &str, payload: &Value) -> anyhow::Result<()> {
        let path = format!("hazard_map/{}", sanitize_document_id(doc_id));
        self.upsert_document(&path, payload).await
    }

    async fn upsert_document(&self, path: &str, payload: &Value) -> anyhow::Result<()> {
        let token = self.access_token().await?;
        let body = firestore_document_body(payload)?;
        let url = format!("{}/{}", self.base_documents_url, path);

        let response = self
            .client
            .patch(url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .context("failed to call Firestore PATCH")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("firestore PATCH failed: status={status} body={text}");
        }

        Ok(())
    }

    async fn create_document(&self, collection_path: &str, payload: &Value) -> anyhow::Result<()> {
        let token = self.access_token().await?;
        let body = firestore_document_body(payload)?;
        let url = format!("{}/{}", self.base_documents_url, collection_path);

        let response = self
            .client
            .post(url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .context("failed to call Firestore POST")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("firestore POST failed: status={status} body={text}");
        }

        Ok(())
    }

    async fn access_token(&self) -> anyhow::Result<String> {
        if let Some(token) = self.static_bearer_token.clone() {
            return Ok(token);
        }

        {
            let guard = self.metadata_token_cache.read().await;
            if let Some(cache) = guard.as_ref() {
                if cache.expires_epoch > now_epoch() + 30.0 {
                    return Ok(cache.access_token.clone());
                }
            }
        }

        let response = self
            .client
            .get("http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token")
            .header("Metadata-Flavor", "Google")
            .send()
            .await
            .context("failed to fetch metadata token")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("metadata token endpoint failed: status={status} body={text}");
        }

        let payload = response
            .json::<MetadataTokenResponse>()
            .await
            .context("failed to parse metadata token response")?;

        let expires_epoch = now_epoch() + payload.expires_in as f64;
        {
            let mut guard = self.metadata_token_cache.write().await;
            *guard = Some(MetadataTokenCache {
                access_token: payload.access_token.clone(),
                expires_epoch,
            });
        }

        Ok(payload.access_token)
    }
}

fn firestore_document_body(payload: &Value) -> anyhow::Result<Value> {
    let Some(obj) = payload.as_object() else {
        anyhow::bail!("Firestore payload must be a JSON object");
    };

    let mut fields = Map::new();
    for (key, value) in obj {
        fields.insert(key.clone(), firestore_value(value));
    }

    Ok(json!({ "fields": fields }))
}

fn firestore_value(value: &Value) -> Value {
    match value {
        Value::Null => json!({ "nullValue": Value::Null }),
        Value::Bool(flag) => json!({ "booleanValue": flag }),
        Value::Number(number) => {
            if let Some(as_i64) = number.as_i64() {
                json!({ "integerValue": as_i64.to_string() })
            } else if let Some(as_u64) = number.as_u64() {
                json!({ "integerValue": as_u64.to_string() })
            } else if let Some(as_f64) = number.as_f64() {
                json!({ "doubleValue": as_f64 })
            } else {
                json!({ "doubleValue": 0.0 })
            }
        }
        Value::String(text) => json!({ "stringValue": text }),
        Value::Array(items) => {
            let values = items.iter().map(firestore_value).collect::<Vec<_>>();
            json!({ "arrayValue": { "values": values } })
        }
        Value::Object(map) => {
            let mut fields = Map::new();
            for (key, item) in map {
                fields.insert(key.clone(), firestore_value(item));
            }
            json!({ "mapValue": { "fields": fields } })
        }
    }
}

fn env_flag(name: &str, default: bool) -> bool {
    let Ok(raw) = std::env::var(name) else {
        return default;
    };

    !matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "off" | "no"
    )
}

fn sanitize_document_id(value: &str) -> String {
    value.trim().replace('/', "_")
}

fn sanitize_doc_id_fragment(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn encode_geohash(lat: f64, lng: f64, precision: usize) -> String {
    let mut lat_interval = [-90.0_f64, 90.0_f64];
    let mut lng_interval = [-180.0_f64, 180.0_f64];
    let mut geohash = String::new();
    let bits = [16_u8, 8, 4, 2, 1];
    let mut bit = 0usize;
    let mut ch = 0usize;
    let mut even = true;

    while geohash.len() < precision {
        if even {
            let mid = (lng_interval[0] + lng_interval[1]) / 2.0;
            if lng > mid {
                ch |= bits[bit] as usize;
                lng_interval[0] = mid;
            } else {
                lng_interval[1] = mid;
            }
        } else {
            let mid = (lat_interval[0] + lat_interval[1]) / 2.0;
            if lat > mid {
                ch |= bits[bit] as usize;
                lat_interval[0] = mid;
            } else {
                lat_interval[1] = mid;
            }
        }

        even = !even;
        if bit < 4 {
            bit += 1;
        } else {
            if let Some(code) = GEOHASH_BASE32.as_bytes().get(ch) {
                geohash.push(*code as char);
            }
            bit = 0;
            ch = 0;
        }
    }

    geohash
}

fn now_epoch() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

fn navigation_mode_str(mode: NavigationMode) -> &'static str {
    match mode {
        NavigationMode::Navigation => "NAVIGATION",
        NavigationMode::Explore => "EXPLORE",
        NavigationMode::Read => "READ",
        NavigationMode::Quiet => "QUIET",
    }
}

fn motion_state_str(state: MotionState) -> &'static str {
    match state {
        MotionState::Stationary => "stationary",
        MotionState::WalkingSlow => "walking_slow",
        MotionState::WalkingFast => "walking_fast",
        MotionState::Running => "running",
    }
}

fn distance_band_for_m(distance_m: f32) -> &'static str {
    if distance_m <= 1.5 {
        "very_close"
    } else if distance_m <= 3.5 {
        "mid"
    } else {
        "far"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stress_override_forces_navigation_temporarily() {
        let store = SessionStore::default();
        let _ = store.set_mode("s1", NavigationMode::Read).await;
        let state = store.apply_stress_mode_override("s1", "panic", 60).await;

        let forced_mode = store
            .get_effective_mode_at("s1", state.mode_override_until_epoch - 1.0)
            .await;
        assert_eq!(forced_mode, NavigationMode::Navigation);

        let restored_mode = store
            .get_effective_mode_at("s1", state.mode_override_until_epoch + 1.0)
            .await;
        assert_eq!(restored_mode, NavigationMode::Read);
    }

    #[tokio::test]
    async fn edge_hazard_expires_after_suppress_window() {
        let store = SessionStore::default();
        let state = store
            .mark_edge_hazard("s2", "EDGE_DROP".to_string(), Some(2))
            .await;

        let active = store
            .is_edge_hazard_active_at("s2", state.edge_hazard_until_epoch - 0.5)
            .await;
        let expired = store
            .is_edge_hazard_active_at("s2", state.edge_hazard_until_epoch + 0.5)
            .await;

        assert!(active);
        assert!(!expired);
    }

    #[tokio::test]
    async fn utterance_rate_limiter_blocks_burst() {
        let store = SessionStore::default();
        let now = 1000.0;

        assert!(store.should_allow_utterance("s3", now, 0.3, 2, 4.0).await);
        assert!(
            !store
                .should_allow_utterance("s3", now + 0.1, 0.3, 2, 4.0)
                .await
        );
        assert!(
            store
                .should_allow_utterance("s3", now + 0.4, 0.3, 2, 4.0)
                .await
        );
        assert!(
            !store
                .should_allow_utterance("s3", now + 0.8, 0.3, 2, 4.0)
                .await
        );
    }

    #[tokio::test]
    async fn crowd_hazard_hints_collect_from_same_geohash_prefix() {
        let store = SessionStore::default();
        let _ = store
            .touch_session("s4", None, Some(10.776), Some(106.700), Some(90.0), true)
            .await;

        store
            .log_hazard(
                "s4",
                "STAIRS_DOWN",
                0.1,
                Some(1.1),
                Some(-1.2),
                0.9,
                Some(4.2),
                Some(true),
                Some("test"),
                "Cau thang di xuong",
            )
            .await;

        let hints = store.get_crowd_hazard_hints(10.7761, 106.7002, 3).await;
        assert!(!hints.is_empty());
    }

    #[tokio::test]
    async fn hazard_events_persist_continuous_fields_not_legacy_distance_category() {
        let store = SessionStore::default();
        store
            .log_hazard(
                "s5",
                "DROP_AHEAD",
                -0.2,
                Some(1.3),
                Some(-1.9),
                0.91,
                Some(4.7),
                Some(true),
                Some("native_depth"),
                "continuous_hazard",
            )
            .await;

        let hazards = store.hazards.read().await;
        let bucket = hazards.get("s5").expect("hazard bucket");
        let event = bucket.last().expect("hazard event");

        assert!(event.get("distance_m").is_some());
        assert!(event.get("relative_velocity_mps").is_some());
        assert!(event.get("hazard_score").is_some());
        assert!(event.get("hard_stop").is_some());
        assert!(event.get("distance").is_none());
    }
}
