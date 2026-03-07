use std::{net::SocketAddr, time::Duration};

use apollos_server::{build_router, AppState};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle, time::timeout};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

fn configure_test_env() {
    std::env::set_var("APP_ENV", "development");
    std::env::set_var("ENABLE_GEMINI_LIVE", "0");
    std::env::set_var("OIDC_ALLOW_INSECURE_DEV_TOKENS", "1");
    std::env::set_var("TWILIO_REQUIRED", "0");
    std::env::set_var("SINGLE_INSTANCE_ONLY", "1");
    std::env::set_var("WS_ALLOW_QUERY_TOKEN", "1");
}

async fn spawn_server(state: AppState) -> (SocketAddr, oneshot::Sender<()>, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let addr = listener.local_addr().expect("local addr");
    let router = build_router(state);
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        let server =
            axum::serve(listener, router.into_make_service()).with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            });
        let _ = server.await;
    });

    (addr, shutdown_tx, handle)
}

async fn issue_ws_access_token(base_url: &str) -> String {
    let client = reqwest::Client::new();
    let exchange = client
        .post(format!("{base_url}/auth/oidc/exchange"))
        .json(&json!({ "id_token": "abcdefghijklmnopqrstuvwxyz-1234567890" }))
        .send()
        .await
        .expect("exchange call")
        .error_for_status()
        .expect("exchange success")
        .json::<Value>()
        .await
        .expect("exchange body");
    let session_token = exchange
        .get("session_token")
        .and_then(Value::as_str)
        .expect("session token");

    let ws = client
        .post(format!("{base_url}/auth/ws-ticket"))
        .json(&json!({ "session_token": session_token }))
        .send()
        .await
        .expect("ws ticket call")
        .error_for_status()
        .expect("ws ticket success")
        .json::<Value>()
        .await
        .expect("ws ticket body");

    ws.get("access_token")
        .and_then(Value::as_str)
        .expect("ws access token")
        .to_string()
}

async fn connect_live_socket(
    addr: SocketAddr,
    session_id: &str,
    ws_access_token: &str,
) -> WsStream {
    let ws_url = format!("ws://{addr}/ws/live/{session_id}?access_token={ws_access_token}");
    let (socket, _) = connect_async(ws_url).await.expect("ws connect");
    socket
}

async fn recv_json_message(ws: &mut WsStream) -> Value {
    let incoming = timeout(Duration::from_secs(3), ws.next())
        .await
        .expect("ws timeout")
        .expect("ws stream item")
        .expect("ws item ok");

    match incoming {
        Message::Text(text) => serde_json::from_str::<Value>(&text).expect("json message"),
        other => panic!("expected text message, got {other:?}"),
    }
}

fn native_frame_payload(
    session_id: &str,
    source: &str,
    sensor_health_score: f32,
    covariance_3x3: [f32; 9],
) -> Value {
    json!({
        "type": "multimodal_frame",
        "session_id": session_id,
        "timestamp_ms": 1_741_324_800_000u64,
        "frame_jpeg_base64": Value::Null,
        "motion_state": "walking_fast",
        "pitch": 0.0,
        "velocity": 1.2,
        "user_text": Value::Null,
        "yaw_delta_deg": 0.0,
        "carry_mode": "necklace",
        "sensor_unavailable": false,
        "lat": 10.776,
        "lng": 106.700,
        "heading_deg": Value::Null,
        "location_accuracy_m": 3.0,
        "location_age_ms": 0,
        "sensor_health": {
            "score": sensor_health_score,
            "flags": ["eskf_degraded"],
            "degraded": true,
            "source": source
        },
        "sensor_uncertainty": {
            "covariance_3x3": covariance_3x3,
            "innovation_norm": 0.8,
            "source": source
        },
        "vision_odometry": {
            "source": source,
            "applied": true,
            "optical_flow_score": 0.77,
            "variance_m2": 1.4,
            "pose_x_m": 0.9,
            "pose_y_m": -0.2
        },
        "cloud_link": {
            "connected": true,
            "rtt_ms": 65.0,
            "source": "native-smoke"
        },
        "edge_semantic_cues": []
    })
}

#[tokio::test]
async fn android_ios_payloads_pass_ws_smoke_and_update_observability() {
    configure_test_env();

    let state = AppState::default();
    let (addr, shutdown_tx, server_handle) = spawn_server(state.clone()).await;
    let base_url = format!("http://{addr}");
    let ws_token = issue_ws_access_token(&base_url).await;
    let session_id = "native-smoke-session";

    let mut socket = connect_live_socket(addr, session_id, &ws_token).await;

    let android_payload = native_frame_payload(
        session_id,
        "android-eskf-runtime-v3",
        0.81,
        [1.0, 0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 9.0],
    );
    socket
        .send(Message::Text(android_payload.to_string().into()))
        .await
        .expect("send android payload");
    let reply = recv_json_message(&mut socket).await;
    assert_eq!(
        reply.get("type").and_then(Value::as_str),
        Some("connection_state")
    );

    let after_android = state.sessions.get_observability(session_id).await;
    assert!((after_android.sensor_health_score - 0.81).abs() < 1e-4);
    assert!(after_android.localization_uncertainty_m > 3.7);
    assert!(after_android.localization_uncertainty_m < 3.8);

    let ios_payload = native_frame_payload(
        session_id,
        "ios-eskf-runtime-v3",
        0.93,
        [0.25, 0.0, 0.0, 0.0, 0.25, 0.0, 0.0, 0.0, 0.36],
    );
    socket
        .send(Message::Text(ios_payload.to_string().into()))
        .await
        .expect("send ios payload");
    let reply = recv_json_message(&mut socket).await;
    assert_eq!(
        reply.get("type").and_then(Value::as_str),
        Some("connection_state")
    );

    let after_ios = state.sessions.get_observability(session_id).await;
    assert!((after_ios.sensor_health_score - 0.93).abs() < 1e-4);
    assert!(after_ios.localization_uncertainty_m > 0.9);
    assert!(after_ios.localization_uncertainty_m < 1.0);

    let _ = socket.close(None).await;
    let _ = shutdown_tx.send(());
    let _ = timeout(Duration::from_secs(3), server_handle).await;
}
