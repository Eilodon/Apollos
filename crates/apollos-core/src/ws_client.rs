use apollos_proto::{
    contracts::{BackendToClientMessage, ClientToBackendMessage},
    transport,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WsTransport {
    #[default]
    Json,
    Protobuf,
}

#[derive(Debug, Clone)]
pub struct WsClientConfig {
    pub endpoint: String,
    pub session_id: String,
    pub transport: WsTransport,
}

impl Default for WsClientConfig {
    fn default() -> Self {
        Self {
            endpoint: "ws://127.0.0.1:8000/ws/live/default".to_string(),
            session_id: "default".to_string(),
            transport: WsTransport::Json,
        }
    }
}

pub fn encode_message_json(message: &ClientToBackendMessage) -> Result<String, serde_json::Error> {
    serde_json::to_string(message)
}

pub fn encode_message_protobuf(
    message: &ClientToBackendMessage,
) -> Result<Vec<u8>, transport::TransportError> {
    transport::encode_client_message(message)
}

pub fn decode_message_protobuf(
    payload: &[u8],
) -> Result<BackendToClientMessage, transport::TransportError> {
    transport::decode_server_message(payload)
}

#[cfg(feature = "net")]
pub mod net {
    use futures_util::{SinkExt, StreamExt};

    use super::{
        decode_message_protobuf, encode_message_json, encode_message_protobuf, WsClientConfig,
        WsTransport,
    };
    use crate::session::SessionState;

    pub async fn connect_and_send(
        config: WsClientConfig,
        state: &SessionState,
    ) -> anyhow::Result<()> {
        let (mut socket, _) = tokio_tungstenite::connect_async(config.endpoint).await?;

        match config.transport {
            WsTransport::Json => {
                let msg = encode_message_json(&state.bootstrap_message())?;
                socket
                    .send(tokio_tungstenite::tungstenite::Message::Text(msg.into()))
                    .await?;
            }
            WsTransport::Protobuf => {
                let msg = encode_message_protobuf(&state.bootstrap_message())?;
                socket
                    .send(tokio_tungstenite::tungstenite::Message::Binary(msg.into()))
                    .await?;
            }
        }

        if let Some(incoming) = socket.next().await {
            let incoming = incoming?;
            if let tokio_tungstenite::tungstenite::Message::Binary(payload) = incoming {
                let _ = decode_message_protobuf(&payload)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use apollos_proto::contracts::{
        ClientToBackendMessage, ConnectionState, ConnectionStateMessage, UserCommandMessage,
    };

    use super::*;

    #[test]
    fn protobuf_roundtrip_with_transport_module() {
        let outbound = ClientToBackendMessage::UserCommand(UserCommandMessage {
            session_id: "s1".to_string(),
            timestamp: "2026-03-05T10:00:00Z".to_string(),
            command: "status".to_string(),
        });
        let encoded = encode_message_protobuf(&outbound).expect("encode should pass");

        let inbound = transport::encode_server_message(&BackendToClientMessage::ConnectionState(
            ConnectionStateMessage {
                state: ConnectionState::Connected,
                detail: Some("ok".to_string()),
            },
        ))
        .expect("encode should pass");

        let decoded = decode_message_protobuf(&inbound).expect("decode should pass");
        assert!(matches!(
            decoded,
            BackendToClientMessage::ConnectionState(_)
        ));
        assert!(!encoded.is_empty());
    }
}
