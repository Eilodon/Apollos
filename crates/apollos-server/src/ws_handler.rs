use std::{collections::HashMap, sync::Arc};

use apollos_proto::{
    contracts::{
        BackendToClientMessage, ClientToBackendMessage, ConnectionState, ConnectionStateMessage,
    },
    transport,
};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use crate::{agent::AgentOrchestrator, auth::ws_auth, AppState};

#[derive(Debug, Clone, Copy)]
pub(crate) enum Channel {
    Live,
    Emergency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SocketEncoding {
    Json,
    Protobuf,
}

pub async fn live_ws_handler(
    Path(session_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(client_id) = authorize_ws(&state, &session_id, &headers, &query).await else {
        return unauthorized("unauthorized websocket");
    };

    let (ws, encoding) = negotiate_live_protocol(ws, &headers);

    ws.on_upgrade(move |socket| {
        ws_loop(
            socket,
            Arc::new(state),
            session_id,
            Channel::Live,
            Some(client_id),
            encoding,
        )
    })
}

pub async fn emergency_ws_handler(
    Path(session_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(client_id) = authorize_ws(&state, &session_id, &headers, &query).await else {
        return unauthorized("unauthorized websocket");
    };

    let (ws, encoding) = negotiate_live_protocol(ws, &headers);

    ws.on_upgrade(move |socket| {
        ws_loop(
            socket,
            Arc::new(state),
            session_id,
            Channel::Emergency,
            Some(client_id),
            encoding,
        )
    })
}

pub async fn help_ws_handler(
    Path(session_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let allow_query_token = ws_auth::resolve_allow_query_token(
        &std::env::var("APP_ENV").unwrap_or_else(|_| "development".to_string()),
        std::env::var("WS_ALLOW_QUERY_TOKEN").ok().as_deref(),
    );

    let Some(token) = ws_auth::extract_ws_token(&headers, &query, allow_query_token) else {
        return unauthorized("missing viewer token");
    };

    let Some(viewer_claims) = state
        .fallback
        .verify_viewer_token(&token, &session_id)
        .await
    else {
        return unauthorized("invalid viewer token");
    };

    let selected_protocol = ws_auth::select_ws_subprotocol(&headers, "apollos.help.v1");
    let ws = if let Some(protocol) = selected_protocol {
        ws.protocols([protocol])
    } else {
        ws
    };

    ws.on_upgrade(move |socket| {
        help_ws_loop(socket, Arc::new(state), session_id, viewer_claims.viewer_id)
    })
}

fn negotiate_live_protocol(
    ws: WebSocketUpgrade,
    headers: &HeaderMap,
) -> (WebSocketUpgrade, SocketEncoding) {
    let protocol_pb = ws_auth::select_ws_subprotocol(headers, "apollos.pb.v1");
    if let Some(protocol) = protocol_pb {
        return (ws.protocols([protocol]), SocketEncoding::Protobuf);
    }

    let protocol_json = ws_auth::select_ws_subprotocol(headers, "apollos.v1");
    if let Some(protocol) = protocol_json {
        return (ws.protocols([protocol]), SocketEncoding::Json);
    }

    (ws, SocketEncoding::Json)
}

async fn authorize_ws(
    state: &AppState,
    session_id: &str,
    headers: &HeaderMap,
    query: &HashMap<String, String>,
) -> Option<String> {
    let allow_query_token = ws_auth::resolve_allow_query_token(
        &std::env::var("APP_ENV").unwrap_or_else(|_| "development".to_string()),
        std::env::var("WS_ALLOW_QUERY_TOKEN").ok().as_deref(),
    );

    let token = ws_auth::extract_ws_token(headers, query, allow_query_token)?;
    let claims = state.broker.verify_ws_ticket(&token).await?;

    if claims.subject.trim().is_empty() {
        return None;
    }

    if session_id.trim().is_empty() {
        return None;
    }

    Some(claims.subject)
}

async fn ws_loop(
    mut socket: WebSocket,
    state: Arc<AppState>,
    session_id: String,
    channel: Channel,
    client_id: Option<String>,
    encoding: SocketEncoding,
) {
    let (tx, mut rx) = mpsc::unbounded_channel::<BackendToClientMessage>();

    let registration = match channel {
        Channel::Live => {
            state
                .ws_registry
                .register_live(&session_id, tx.clone(), client_id)
                .await
        }
        Channel::Emergency => {
            state
                .ws_registry
                .register_emergency(&session_id, tx.clone(), client_id)
                .await
        }
    };

    let connection_id = match registration {
        Ok(connection_id) => connection_id,
        Err(detail) => {
            let payload = BackendToClientMessage::ConnectionState(ConnectionStateMessage {
                state: ConnectionState::Disconnected,
                detail: Some(detail),
            });
            let _ = socket
                .send(Message::Text(
                    serde_json::to_string(&payload)
                        .unwrap_or_else(|_| "{\"type\":\"connection_state\"}".to_string())
                        .into(),
                ))
                .await;
            let _ = socket.close().await;
            return;
        }
    };

    let (mut ws_sender, mut ws_receiver) = socket.split();

    let writer = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            let outgoing = serialize_outgoing(&message, encoding);
            let Ok(outgoing) = outgoing else {
                continue;
            };

            if ws_sender.send(outgoing).await.is_err() {
                break;
            }
        }
    });

    let orchestrator = AgentOrchestrator::new(channel);

    while let Some(incoming) = ws_receiver.next().await {
        let Ok(message) = incoming else {
            break;
        };

        let parsed = parse_incoming(message, encoding);
        let Ok(client_message) = parsed else {
            let _ = tx.send(BackendToClientMessage::ConnectionState(
                ConnectionStateMessage {
                    state: ConnectionState::Degraded,
                    detail: Some("invalid_payload".to_string()),
                },
            ));
            continue;
        };

        if client_message_session_id(&client_message) != session_id {
            let _ = tx.send(BackendToClientMessage::ConnectionState(
                ConnectionStateMessage {
                    state: ConnectionState::Degraded,
                    detail: Some("session_id_mismatch".to_string()),
                },
            ));
            continue;
        }

        if let Some(reply) = orchestrator.route_message(&state, client_message).await {
            let _ = tx.send(reply);
        }
    }

    match channel {
        Channel::Live => {
            let _ = state
                .ws_registry
                .unregister_live(&session_id, Some(&connection_id))
                .await;
            state.gemini.close_live_session(&session_id).await;
        }
        Channel::Emergency => {
            let _ = state
                .ws_registry
                .unregister_emergency(&session_id, Some(&connection_id))
                .await;
        }
    }

    writer.abort();
}

fn parse_incoming(
    message: Message,
    encoding: SocketEncoding,
) -> Result<ClientToBackendMessage, transport::TransportError> {
    match (encoding, message) {
        (SocketEncoding::Json, Message::Text(text)) => {
            serde_json::from_str::<ClientToBackendMessage>(&text)
                .map_err(|_| transport::TransportError::MissingPayload("json_parse"))
        }
        (SocketEncoding::Json, Message::Binary(bytes)) => transport::decode_client_message(&bytes),
        (SocketEncoding::Protobuf, Message::Binary(bytes)) => {
            transport::decode_client_message(&bytes)
        }
        (SocketEncoding::Protobuf, Message::Text(text)) => {
            serde_json::from_str::<ClientToBackendMessage>(&text)
                .map_err(|_| transport::TransportError::MissingPayload("json_parse"))
        }
        (_, _) => Err(transport::TransportError::MissingPayload(
            "unsupported_ws_message",
        )),
    }
}

fn serialize_outgoing(
    message: &BackendToClientMessage,
    encoding: SocketEncoding,
) -> Result<Message, transport::TransportError> {
    match encoding {
        SocketEncoding::Json => {
            let text = serde_json::to_string(message)
                .map_err(|_| transport::TransportError::MissingPayload("json_serialize"))?;
            Ok(Message::Text(text.into()))
        }
        SocketEncoding::Protobuf => {
            let bytes = transport::encode_server_message(message)?;
            Ok(Message::Binary(bytes.into()))
        }
    }
}

async fn help_ws_loop(
    socket: WebSocket,
    state: Arc<AppState>,
    session_id: String,
    viewer_id: String,
) {
    let (tx, mut rx) = mpsc::unbounded_channel::<BackendToClientMessage>();
    state
        .ws_registry
        .register_help_viewer(&session_id, &viewer_id, tx)
        .await;

    let (mut ws_sender, mut ws_receiver) = socket.split();

    let writer = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            let Ok(serialized) = serde_json::to_string(&message) else {
                continue;
            };

            if ws_sender
                .send(Message::Text(serialized.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    while let Some(incoming) = ws_receiver.next().await {
        if incoming.is_err() {
            break;
        }
    }

    state
        .ws_registry
        .unregister_help_viewer(&session_id, &viewer_id)
        .await;

    writer.abort();
}

fn unauthorized(detail: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        axum::Json(serde_json::json!({ "detail": detail })),
    )
        .into_response()
}

fn client_message_session_id(message: &ClientToBackendMessage) -> &str {
    match message {
        ClientToBackendMessage::MultimodalFrame(payload) => &payload.session_id,
        ClientToBackendMessage::AudioChunk(payload) => &payload.session_id,
        ClientToBackendMessage::UserCommand(payload) => &payload.session_id,
        ClientToBackendMessage::EdgeHazard(payload) => &payload.session_id,
    }
}

pub fn server_message_to_text(
    message: &BackendToClientMessage,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use apollos_proto::contracts::{ClientToBackendMessage, UserCommandMessage};

    #[test]
    fn parses_json_when_encoding_json() {
        let payload =
            serde_json::to_string(&ClientToBackendMessage::UserCommand(UserCommandMessage {
                session_id: "s1".to_string(),
                timestamp: "2026-03-05T10:00:00Z".to_string(),
                command: "help".to_string(),
            }))
            .expect("serialize");

        let parsed = parse_incoming(Message::Text(payload.into()), SocketEncoding::Json)
            .expect("parse should pass");

        assert!(matches!(parsed, ClientToBackendMessage::UserCommand(_)));
    }

    #[test]
    fn parses_protobuf_when_encoding_protobuf() {
        let message = ClientToBackendMessage::UserCommand(UserCommandMessage {
            session_id: "s2".to_string(),
            timestamp: "2026-03-05T10:00:00Z".to_string(),
            command: "status".to_string(),
        });
        let encoded = transport::encode_client_message(&message).expect("encode");

        let parsed = parse_incoming(Message::Binary(encoded.into()), SocketEncoding::Protobuf)
            .expect("parse should pass");

        assert_eq!(parsed, message);
    }
}
