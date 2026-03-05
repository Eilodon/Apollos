use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::Context;
use apollos_proto::contracts::{
    AssistantAudioMessage, AssistantTextMessage, BackendToClientMessage, ConnectionState,
    ConnectionStateMessage, DistanceCategory, HardStopMessage, HumanHelpSessionMessage,
    NavigationMode, SemanticCue, SemanticCueMessage,
};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Map, Value};
use tokio::sync::{mpsc, RwLock};
use tokio_tungstenite::tungstenite::Message;

use crate::{prompts::SYSTEM_PROMPT, AppState};

type LiveSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

#[derive(Debug, Clone)]
struct LiveSessionHandle {
    tx: mpsc::UnboundedSender<LiveOutgoing>,
}

#[derive(Debug)]
enum LiveOutgoing {
    ClientContent {
        parts: Vec<Value>,
        turn_complete: bool,
    },
    RealtimeAudio {
        pcm16_base64: String,
    },
    Close,
}

#[derive(Debug, Clone)]
struct GeminiToolCall {
    id: String,
    name: String,
    args: Value,
}

#[derive(Debug, Clone)]
struct LiveSessionRunner {
    session_id: String,
    api_key: String,
    model: String,
    temperature: f32,
    live_endpoint_base: String,
    sessions: crate::session::SessionStore,
    ws_registry: crate::ws_registry::WebSocketRegistry,
    fallback: crate::human_fallback::HumanFallbackService,
}

#[derive(Debug, Clone)]
pub struct GeminiBridge {
    client: reqwest::Client,
    api_key: Option<String>,
    model: String,
    endpoint_base: String,
    temperature: f32,
    live_enabled: bool,
    live_endpoint_base: String,
    live_sessions: Arc<RwLock<HashMap<String, LiveSessionHandle>>>,
}

impl Default for GeminiBridge {
    fn default() -> Self {
        let timeout_seconds = std::env::var("GEMINI_HTTP_TIMEOUT_S")
            .ok()
            .and_then(|raw| raw.parse::<u64>().ok())
            .unwrap_or(20)
            .max(5);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_seconds))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let api_key = std::env::var("GEMINI_API_KEY")
            .ok()
            .or_else(|| std::env::var("GOOGLE_API_KEY").ok())
            .filter(|value| !value.trim().is_empty());

        let model =
            std::env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemini-2.5-flash".to_string());

        let endpoint_base = std::env::var("GEMINI_ENDPOINT_BASE")
            .unwrap_or_else(|_| "https://generativelanguage.googleapis.com/v1beta".to_string());

        let temperature = std::env::var("GEMINI_TEMPERATURE")
            .ok()
            .and_then(|raw| raw.parse::<f32>().ok())
            .unwrap_or(0.2)
            .clamp(0.0, 1.0);

        let live_endpoint_base = std::env::var("GEMINI_LIVE_ENDPOINT_BASE").unwrap_or_else(|_| {
            "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent".to_string()
        });

        let live_enabled = env_flag("ENABLE_GEMINI_LIVE", true) && api_key.is_some();

        Self {
            client,
            api_key,
            model,
            endpoint_base,
            temperature,
            live_enabled,
            live_endpoint_base,
            live_sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl GeminiBridge {
    pub fn live_enabled(&self) -> bool {
        self.live_enabled
    }

    pub async fn infer_text(&self, prompt: &str) -> anyhow::Result<String> {
        let trimmed = prompt.trim();
        if trimmed.is_empty() {
            anyhow::bail!("gemini prompt cannot be empty");
        }

        if self.api_key.is_none() {
            return Ok(format!("[gemini_stub] {trimmed}"));
        }

        let text = self
            .infer_text_via_generate_content(trimmed)
            .await
            .context("gemini REST inference failed")?;

        if text.trim().is_empty() {
            anyhow::bail!("gemini returned empty text");
        }

        Ok(text)
    }

    pub async fn forward_multimodal_frame(
        &self,
        state: &AppState,
        frame: &apollos_proto::contracts::MultimodalFrameMessage,
    ) -> anyhow::Result<()> {
        if !self.live_enabled {
            return Ok(());
        }

        let mut parts = Vec::new();

        if let Some(frame_jpeg_base64) = frame.frame_jpeg_base64.as_ref() {
            let trimmed = frame_jpeg_base64.trim();
            if !trimmed.is_empty() {
                parts.push(json!({
                    "inlineData": {
                        "mimeType": "image/jpeg",
                        "data": trimmed,
                    }
                }));
            }
        }

        let mode = state.sessions.get_effective_mode(&frame.session_id).await;
        let observability = state.sessions.get_observability(&frame.session_id).await;
        let motion_hint = format!(
            "[KINEMATIC: motion={:?}; pitch={:.1}; velocity={:.2}; mode={:?}; sensor_health={:.2}; degraded={}]",
            frame.motion_state,
            frame.pitch,
            frame.velocity,
            mode,
            observability.sensor_health_score,
            if observability.degraded_mode { "1" } else { "0" },
        );

        parts.push(json!({ "text": motion_hint }));

        self.send_client_content(&frame.session_id, state, parts, false)
            .await?;

        if let Some(user_text) = frame.user_text.as_deref() {
            let trimmed = user_text.trim();
            if !trimmed.is_empty() {
                self.forward_user_command_by_session(&frame.session_id, state, trimmed)
                    .await?;
            }
        }

        Ok(())
    }

    pub async fn forward_audio_chunk(
        &self,
        state: &AppState,
        chunk: &apollos_proto::contracts::AudioChunkMessage,
    ) -> anyhow::Result<()> {
        if !self.live_enabled {
            return Ok(());
        }

        let trimmed = chunk.audio_chunk_pcm16.trim();
        if trimmed.is_empty() {
            return Ok(());
        }

        let handle = self.ensure_live_session(state, &chunk.session_id).await?;
        handle
            .tx
            .send(LiveOutgoing::RealtimeAudio {
                pcm16_base64: trimmed.to_string(),
            })
            .map_err(|_| anyhow::anyhow!("gemini live outbound channel closed"))?;
        Ok(())
    }

    pub async fn forward_user_command(
        &self,
        state: &AppState,
        command: &apollos_proto::contracts::UserCommandMessage,
    ) -> anyhow::Result<()> {
        self.forward_user_command_by_session(&command.session_id, state, &command.command)
            .await
    }

    pub async fn close_live_session(&self, session_id: &str) {
        let handle = {
            let mut guard = self.live_sessions.write().await;
            guard.remove(session_id)
        };

        if let Some(handle) = handle {
            let _ = handle.tx.send(LiveOutgoing::Close);
        }
    }

    async fn forward_user_command_by_session(
        &self,
        session_id: &str,
        state: &AppState,
        text: &str,
    ) -> anyhow::Result<()> {
        if !self.live_enabled {
            return Ok(());
        }

        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(());
        }

        self.send_client_content(session_id, state, vec![json!({ "text": trimmed })], true)
            .await
    }

    async fn send_client_content(
        &self,
        session_id: &str,
        state: &AppState,
        parts: Vec<Value>,
        turn_complete: bool,
    ) -> anyhow::Result<()> {
        let handle = self.ensure_live_session(state, session_id).await?;
        handle
            .tx
            .send(LiveOutgoing::ClientContent {
                parts,
                turn_complete,
            })
            .map_err(|_| anyhow::anyhow!("gemini live outbound channel closed"))
    }

    async fn ensure_live_session(
        &self,
        state: &AppState,
        session_id: &str,
    ) -> anyhow::Result<LiveSessionHandle> {
        if let Some(existing) = self.live_sessions.read().await.get(session_id).cloned() {
            return Ok(existing);
        }

        let api_key = self
            .api_key
            .clone()
            .ok_or_else(|| anyhow::anyhow!("missing GEMINI_API_KEY/GOOGLE_API_KEY"))?;

        let mut guard = self.live_sessions.write().await;
        if let Some(existing) = guard.get(session_id).cloned() {
            return Ok(existing);
        }

        let (tx, rx) = mpsc::unbounded_channel();
        let handle = LiveSessionHandle { tx: tx.clone() };

        guard.insert(session_id.to_string(), handle.clone());
        drop(guard);

        let runner = LiveSessionRunner {
            session_id: session_id.to_string(),
            api_key,
            model: self.model.clone(),
            temperature: self.temperature,
            live_endpoint_base: self.live_endpoint_base.clone(),
            sessions: state.sessions.clone(),
            ws_registry: state.ws_registry.clone(),
            fallback: state.fallback.clone(),
        };

        let live_sessions = Arc::clone(&self.live_sessions);
        let session_key = session_id.to_string();
        tokio::spawn(async move {
            if let Err(error) = runner.run(rx).await {
                tracing::warn!(
                    session_id = %session_key,
                    error = %error,
                    "gemini live runner exited with error"
                );
            }

            let mut guard = live_sessions.write().await;
            guard.remove(&session_key);
        });

        Ok(handle)
    }

    async fn infer_text_via_generate_content(&self, prompt: &str) -> anyhow::Result<String> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing API key"))?;

        let url = format!(
            "{}/models/{}:generateContent?key={}",
            self.endpoint_base, self.model, api_key
        );

        let request_payload = json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": prompt }]
            }],
            "generationConfig": {
                "temperature": self.temperature
            }
        });

        let response = self
            .client
            .post(url)
            .json(&request_payload)
            .send()
            .await
            .context("failed to call gemini endpoint")?;

        let status = response.status();
        let body = response
            .json::<serde_json::Value>()
            .await
            .context("failed to parse gemini response")?;

        if !status.is_success() {
            anyhow::bail!("gemini http status={} body={}", status, body);
        }

        let text = body
            .get("candidates")
            .and_then(|value| value.as_array())
            .and_then(|candidates| candidates.first())
            .and_then(|candidate| candidate.get("content"))
            .and_then(|content| content.get("parts"))
            .and_then(|parts| parts.as_array())
            .and_then(|parts| parts.iter().find_map(|part| part.get("text")))
            .and_then(|text| text.as_str())
            .unwrap_or_default()
            .trim()
            .to_string();

        if text.is_empty() {
            anyhow::bail!("gemini returned no text candidate: {}", body);
        }

        Ok(text)
    }
}

impl LiveSessionRunner {
    async fn run(
        &self,
        mut outbound_rx: mpsc::UnboundedReceiver<LiveOutgoing>,
    ) -> anyhow::Result<()> {
        let result = self.run_inner(&mut outbound_rx).await;

        if let Err(error) = &result {
            self.emit_connection_state(
                ConnectionState::Reconnecting,
                format!("gemini_live_error: {error}"),
            )
            .await;
        } else {
            self.emit_connection_state(ConnectionState::Disconnected, "gemini_live_closed")
                .await;
        }

        result
    }

    async fn run_inner(
        &self,
        outbound_rx: &mut mpsc::UnboundedReceiver<LiveOutgoing>,
    ) -> anyhow::Result<()> {
        let ws_url = format!("{}?key={}", self.live_endpoint_base, self.api_key);
        let (mut socket, _) = tokio_tungstenite::connect_async(ws_url)
            .await
            .context("failed to open gemini live websocket")?;

        send_ws_json(
            &mut socket,
            &build_live_setup_payload(&self.model, self.temperature),
        )
        .await
        .context("failed to send gemini live setup")?;

        self.emit_connection_state(ConnectionState::Connected, "gemini_live_connected")
            .await;

        loop {
            tokio::select! {
                outgoing = outbound_rx.recv() => {
                    let Some(outgoing) = outgoing else {
                        break;
                    };

                    match outgoing {
                        LiveOutgoing::ClientContent { parts, turn_complete } => {
                            let payload = json!({
                                "clientContent": {
                                    "turns": [{
                                        "role": "user",
                                        "parts": parts,
                                    }],
                                    "turnComplete": turn_complete,
                                }
                            });
                            send_ws_json(&mut socket, &payload).await?;
                        }
                        LiveOutgoing::RealtimeAudio { pcm16_base64 } => {
                            let payload = json!({
                                "realtimeInput": {
                                    "mediaChunks": [{
                                        "mimeType": "audio/pcm;rate=16000",
                                        "data": pcm16_base64,
                                    }]
                                }
                            });
                            send_ws_json(&mut socket, &payload).await?;
                        }
                        LiveOutgoing::Close => {
                            let _ = socket.close(None).await;
                            return Ok(());
                        }
                    }
                }
                incoming = socket.next() => {
                    let Some(incoming) = incoming else {
                        break;
                    };

                    match incoming {
                        Ok(message) => {
                            if matches!(message, Message::Close(_)) {
                                break;
                            }

                            let Some(payload) = parse_ws_json(message) else {
                                continue;
                            };

                            self.handle_server_payload(&mut socket, &payload).await?;
                        }
                        Err(error) => {
                            return Err(anyhow::anyhow!("gemini live websocket stream error: {error}"));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn emit_connection_state(&self, state: ConnectionState, detail: impl Into<String>) {
        let _ = self
            .ws_registry
            .send_live(
                &self.session_id,
                BackendToClientMessage::ConnectionState(ConnectionStateMessage {
                    state,
                    detail: Some(detail.into()),
                }),
            )
            .await;
    }

    async fn handle_server_payload(
        &self,
        socket: &mut LiveSocket,
        payload: &Value,
    ) -> anyhow::Result<()> {
        for text in extract_texts(payload) {
            let _ = self
                .ws_registry
                .send_live(
                    &self.session_id,
                    BackendToClientMessage::AssistantText(AssistantTextMessage {
                        session_id: self.session_id.clone(),
                        timestamp: Utc::now().to_rfc3339(),
                        text,
                    }),
                )
                .await;
        }

        for chunk in extract_audio_chunks(payload) {
            let _ = self
                .ws_registry
                .send_live(
                    &self.session_id,
                    BackendToClientMessage::AssistantAudio(AssistantAudioMessage {
                        session_id: self.session_id.clone(),
                        timestamp: Utc::now().to_rfc3339(),
                        pcm24: None,
                        pcm16: Some(chunk),
                        hazard_position_x: None,
                    }),
                )
                .await;
        }

        let tool_calls = extract_tool_calls(payload);
        if !tool_calls.is_empty() {
            self.handle_tool_calls(socket, tool_calls).await?;
        }

        Ok(())
    }

    async fn handle_tool_calls(
        &self,
        socket: &mut LiveSocket,
        tool_calls: Vec<GeminiToolCall>,
    ) -> anyhow::Result<()> {
        let mut function_responses = Vec::new();

        for call in tool_calls {
            let args = normalize_tool_args(call.args);
            let result = self.dispatch_tool_call(&call.name, &args).await;

            function_responses.push(json!({
                "id": call.id,
                "name": call.name,
                "response": result,
            }));
        }

        if !function_responses.is_empty() {
            let payload = json!({
                "toolResponse": {
                    "functionResponses": function_responses,
                }
            });
            send_ws_json(socket, &payload).await?;
        }

        Ok(())
    }

    async fn dispatch_tool_call(&self, name: &str, args: &Map<String, Value>) -> Value {
        match name {
            "log_hazard_event" => self.tool_log_hazard_event(args).await,
            "set_navigation_mode" => self.tool_set_navigation_mode(args).await,
            "log_emotion_event" => self.tool_log_emotion_event(args).await,
            "escalate_mode_if_stressed" => self.tool_escalate_mode_if_stressed(args).await,
            "identify_location" => self.tool_identify_location(args).await,
            "get_context_summary" => self.tool_get_context_summary().await,
            "request_human_help" => self.tool_request_human_help().await,
            _ => json!({
                "ok": false,
                "error": format!("unknown_tool:{name}"),
            }),
        }
    }

    async fn tool_log_hazard_event(&self, args: &Map<String, Value>) -> Value {
        let hazard_type =
            arg_str(args, "hazard_type").unwrap_or_else(|| "UNKNOWN_HAZARD".to_string());
        let position_x = arg_f32(args, "position_x").unwrap_or(0.0).clamp(-1.0, 1.0);
        let confidence = arg_f32(args, "confidence").unwrap_or(0.7).clamp(0.0, 1.0);
        let description = arg_str(args, "description").unwrap_or_default();
        let distance = parse_distance_category(
            arg_str(args, "distance_category")
                .or_else(|| arg_str(args, "distance"))
                .as_deref(),
        );

        self.sessions
            .mark_edge_hazard(&self.session_id, hazard_type.clone(), Some(3))
            .await;
        self.sessions
            .log_hazard(
                &self.session_id,
                &hazard_type,
                position_x,
                distance,
                confidence,
                description.as_str(),
            )
            .await;

        self.ws_registry
            .emit_hard_stop(
                &self.session_id,
                BackendToClientMessage::HardStop(HardStopMessage {
                    position_x,
                    distance,
                    hazard_type: hazard_type.clone(),
                    confidence,
                    ts: Some(Utc::now().to_rfc3339()),
                }),
            )
            .await;

        let _ = self
            .ws_registry
            .send_live(
                &self.session_id,
                BackendToClientMessage::SemanticCue(SemanticCueMessage {
                    cue: SemanticCue::ApproachingObject,
                    position_x: Some(position_x),
                }),
            )
            .await;

        json!({
            "ok": true,
            "hazard_type": hazard_type,
            "position_x": position_x,
            "confidence": confidence,
            "distance": distance_category_str(distance),
        })
    }

    async fn tool_set_navigation_mode(&self, args: &Map<String, Value>) -> Value {
        let Some(mode_raw) = arg_str(args, "mode") else {
            return json!({"ok": false, "error": "mode_missing"});
        };

        let Some(mode) = parse_navigation_mode(&mode_raw) else {
            return json!({"ok": false, "error": "mode_invalid", "mode": mode_raw});
        };

        self.sessions.set_mode(&self.session_id, mode).await;

        json!({
            "ok": true,
            "mode": navigation_mode_str(mode),
        })
    }

    async fn tool_log_emotion_event(&self, args: &Map<String, Value>) -> Value {
        let state = arg_str(args, "state").unwrap_or_else(|| "unknown".to_string());
        let confidence = arg_f32(args, "confidence").unwrap_or(0.0).clamp(0.0, 1.0);
        self.sessions
            .log_emotion(&self.session_id, &state, confidence)
            .await;

        json!({
            "ok": true,
            "state": state,
            "confidence": confidence,
        })
    }

    async fn tool_escalate_mode_if_stressed(&self, args: &Map<String, Value>) -> Value {
        let state = arg_str(args, "state").unwrap_or_else(|| "unknown".to_string());
        let confidence = arg_f32(args, "confidence").unwrap_or(0.0).clamp(0.0, 1.0);

        let lowered = state.to_ascii_lowercase();
        let stressed = lowered.contains("stress")
            || lowered.contains("panic")
            || lowered.contains("fear")
            || lowered.contains("anxious");

        if stressed && confidence >= 0.65 {
            self.sessions
                .apply_stress_mode_override(&self.session_id, "emotion_stress", 120)
                .await;

            return json!({
                "ok": true,
                "escalated": true,
                "mode": "NAVIGATION",
                "reason": "stress_detected",
            });
        }

        json!({
            "ok": true,
            "escalated": false,
        })
    }

    async fn tool_identify_location(&self, args: &Map<String, Value>) -> Value {
        let lat = arg_f64(args, "lat");
        let lng = arg_f64(args, "lng");
        let heading_deg = arg_f64(args, "heading_deg");

        if lat.is_none() || lng.is_none() {
            return json!({
                "ok": false,
                "error": "missing_coordinates",
            });
        }

        json!({
            "ok": true,
            "name": "location_hint",
            "relevant_info": "Use camera + edge hazards for local obstacle confirmation.",
            "lat": lat,
            "lng": lng,
            "heading_deg": heading_deg,
        })
    }

    async fn tool_get_context_summary(&self) -> Value {
        let summary = self.sessions.get_context_summary(&self.session_id).await;
        json!({
            "ok": true,
            "summary": summary,
        })
    }

    async fn tool_request_human_help(&self) -> Value {
        let help = self
            .fallback
            .create_help_session(&self.session_id, "gemini_tool")
            .await;

        let help_payload = HumanHelpSessionMessage {
            session_id: help.session_id.clone(),
            timestamp: help.timestamp.clone(),
            help_link: help.help_link.clone(),
            rtc: help.rtc.clone(),
        };

        let _ = self
            .ws_registry
            .send_live(
                &self.session_id,
                BackendToClientMessage::HumanHelpSession(help_payload),
            )
            .await;

        json!({
            "ok": true,
            "help_link": help.help_link,
            "rtc": serde_json::to_value(help.rtc).unwrap_or_else(|_| json!({})),
        })
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

fn build_live_setup_payload(model: &str, temperature: f32) -> Value {
    json!({
        "setup": {
            "model": normalize_live_model_name(model),
            "generationConfig": {
                "responseModalities": ["TEXT", "AUDIO"],
                "temperature": temperature,
            },
            "systemInstruction": {
                "parts": [
                    {
                        "text": SYSTEM_PROMPT,
                    }
                ]
            },
            "tools": [
                {
                    "functionDeclarations": live_function_declarations(),
                }
            ],
        }
    })
}

fn normalize_live_model_name(model: &str) -> String {
    let trimmed = model.trim();
    if trimmed.starts_with("models/") {
        trimmed.to_string()
    } else {
        format!("models/{trimmed}")
    }
}

fn live_function_declarations() -> Vec<Value> {
    vec![
        json!({
            "name": "log_hazard_event",
            "description": "Trigger immediate HARD_STOP and log detected hazard.",
            "parameters": {
                "type": "object",
                "properties": {
                    "hazard_type": {"type": "string"},
                    "position_x": {"type": "number", "minimum": -1.0, "maximum": 1.0},
                    "distance_category": {"type": "string", "enum": ["very_close", "mid", "far"]},
                    "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0},
                    "description": {"type": "string"},
                    "session_id": {"type": "string"}
                },
                "required": ["hazard_type", "position_x", "distance_category", "confidence"]
            }
        }),
        json!({
            "name": "set_navigation_mode",
            "description": "Switch navigation mode NAVIGATION/EXPLORE/READ/QUIET.",
            "parameters": {
                "type": "object",
                "properties": {
                    "mode": {
                        "type": "string",
                        "enum": ["NAVIGATION", "EXPLORE", "READ", "QUIET"]
                    }
                },
                "required": ["mode"]
            }
        }),
        json!({
            "name": "log_emotion_event",
            "description": "Log detected emotion state for analytics and adaptive tone.",
            "parameters": {
                "type": "object",
                "properties": {
                    "state": {"type": "string"},
                    "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0}
                },
                "required": ["state", "confidence"]
            }
        }),
        json!({
            "name": "escalate_mode_if_stressed",
            "description": "Escalate mode when distress is detected.",
            "parameters": {
                "type": "object",
                "properties": {
                    "state": {"type": "string"},
                    "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0},
                    "current_mode": {"type": "string"}
                }
            }
        }),
        json!({
            "name": "identify_location",
            "description": "Identify relevant nearby location context.",
            "parameters": {
                "type": "object",
                "properties": {
                    "lat": {"type": "number"},
                    "lng": {"type": "number"},
                    "heading_deg": {"type": "number"}
                },
                "required": ["lat", "lng"]
            }
        }),
        json!({
            "name": "get_context_summary",
            "description": "Get session context summary for reconnect/resume.",
            "parameters": {"type": "object", "properties": {}}
        }),
        json!({
            "name": "request_human_help",
            "description": "Request human assistance and return shareable support link.",
            "parameters": {"type": "object", "properties": {}}
        }),
    ]
}

async fn send_ws_json(socket: &mut LiveSocket, payload: &Value) -> anyhow::Result<()> {
    socket
        .send(Message::Text(payload.to_string()))
        .await
        .context("failed to send gemini websocket message")
}

fn parse_ws_json(message: Message) -> Option<Value> {
    match message {
        Message::Text(text) => serde_json::from_str(&text).ok(),
        Message::Binary(bytes) => {
            let text = String::from_utf8(bytes.to_vec()).ok()?;
            serde_json::from_str(&text).ok()
        }
        _ => None,
    }
}

fn extract_texts(payload: &Value) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    let mut push = |candidate: Option<&str>| {
        if let Some(value) = candidate {
            let normalized = value.trim();
            if !normalized.is_empty() && seen.insert(normalized.to_string()) {
                out.push(normalized.to_string());
            }
        }
    };

    push(payload.get("text").and_then(Value::as_str));

    if let Some(server_content) = field(payload, "server_content", "serverContent") {
        if let Some(transcription) = field(
            server_content,
            "output_transcription",
            "outputTranscription",
        ) {
            push(transcription.get("text").and_then(Value::as_str));
        }

        if let Some(model_turn) = field(server_content, "model_turn", "modelTurn") {
            if let Some(parts) = model_turn.get("parts").and_then(Value::as_array) {
                for part in parts {
                    push(part.get("text").and_then(Value::as_str));
                }
            }
        }
    }

    out
}

fn extract_audio_chunks(payload: &Value) -> Vec<String> {
    let mut chunks = Vec::new();

    if let Some(data) = payload.get("data").and_then(Value::as_str) {
        let trimmed = data.trim();
        if !trimmed.is_empty() {
            chunks.push(trimmed.to_string());
        }
    }

    if let Some(server_content) = field(payload, "server_content", "serverContent") {
        if let Some(model_turn) = field(server_content, "model_turn", "modelTurn") {
            if let Some(parts) = model_turn.get("parts").and_then(Value::as_array) {
                for part in parts {
                    if let Some(inline_data) = field(part, "inline_data", "inlineData") {
                        if let Some(data) = inline_data.get("data").and_then(Value::as_str) {
                            let trimmed = data.trim();
                            if !trimmed.is_empty() {
                                chunks.push(trimmed.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    chunks
}

fn extract_tool_calls(payload: &Value) -> Vec<GeminiToolCall> {
    let mut calls = Vec::new();

    let Some(tool_call) = field(payload, "tool_call", "toolCall") else {
        return calls;
    };

    let Some(function_calls) = field(tool_call, "function_calls", "functionCalls") else {
        return calls;
    };

    let Some(array) = function_calls.as_array() else {
        return calls;
    };

    for item in array {
        let Some(obj) = item.as_object() else {
            continue;
        };

        let name = obj
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();

        if name.is_empty() {
            continue;
        }

        let id = obj
            .get("id")
            .or_else(|| obj.get("call_id"))
            .or_else(|| obj.get("callId"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();

        let args = obj
            .get("args")
            .or_else(|| obj.get("arguments"))
            .cloned()
            .unwrap_or_else(|| json!({}));

        calls.push(GeminiToolCall { id, name, args });
    }

    calls
}

fn normalize_tool_args(raw_args: Value) -> Map<String, Value> {
    if let Some(object) = raw_args.as_object() {
        return object.clone();
    }

    if let Some(raw_text) = raw_args.as_str() {
        if let Ok(parsed) = serde_json::from_str::<Value>(raw_text) {
            if let Some(object) = parsed.as_object() {
                return object.clone();
            }
            let mut map = Map::new();
            map.insert("value".to_string(), parsed);
            return map;
        }

        let mut map = Map::new();
        map.insert("raw".to_string(), Value::String(raw_text.to_string()));
        return map;
    }

    let mut map = Map::new();
    map.insert("value".to_string(), raw_args);
    map
}

fn field<'a>(value: &'a Value, snake: &str, camel: &str) -> Option<&'a Value> {
    value.get(snake).or_else(|| value.get(camel))
}

fn arg_str(args: &Map<String, Value>, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn arg_f32(args: &Map<String, Value>, key: &str) -> Option<f32> {
    args.get(key)
        .and_then(Value::as_f64)
        .map(|value| value as f32)
}

fn arg_f64(args: &Map<String, Value>, key: &str) -> Option<f64> {
    args.get(key).and_then(Value::as_f64)
}

fn parse_navigation_mode(raw: &str) -> Option<NavigationMode> {
    match raw.trim().to_ascii_uppercase().as_str() {
        "NAVIGATION" => Some(NavigationMode::Navigation),
        "EXPLORE" => Some(NavigationMode::Explore),
        "READ" => Some(NavigationMode::Read),
        "QUIET" => Some(NavigationMode::Quiet),
        _ => None,
    }
}

fn navigation_mode_str(mode: NavigationMode) -> &'static str {
    match mode {
        NavigationMode::Navigation => "NAVIGATION",
        NavigationMode::Explore => "EXPLORE",
        NavigationMode::Read => "READ",
        NavigationMode::Quiet => "QUIET",
    }
}

fn parse_distance_category(raw: Option<&str>) -> DistanceCategory {
    match raw.unwrap_or_default().trim().to_ascii_lowercase().as_str() {
        "very_close" | "veryclose" => DistanceCategory::VeryClose,
        "far" => DistanceCategory::Far,
        _ => DistanceCategory::Mid,
    }
}

fn distance_category_str(category: DistanceCategory) -> &'static str {
    match category {
        DistanceCategory::VeryClose => "very_close",
        DistanceCategory::Mid => "mid",
        DistanceCategory::Far => "far",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn falls_back_to_stub_when_api_key_missing() {
        let bridge = GeminiBridge {
            api_key: None,
            ..GeminiBridge::default()
        };

        let result = bridge
            .infer_text("xin chao")
            .await
            .expect("stub should work");

        assert!(result.contains("gemini_stub"));
    }

    #[test]
    fn extracts_tool_calls_from_live_payload() {
        let payload = json!({
            "toolCall": {
                "functionCalls": [
                    {
                        "id": "c1",
                        "name": "set_navigation_mode",
                        "args": {"mode": "READ"}
                    }
                ]
            }
        });

        let calls = extract_tool_calls(&payload);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "c1");
        assert_eq!(calls[0].name, "set_navigation_mode");
    }
}
