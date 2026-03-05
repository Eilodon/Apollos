use std::collections::HashMap;

use axum::http::HeaderMap;
use base64::Engine;

pub fn resolve_allow_query_token(app_env: &str, configured_value: Option<&str>) -> bool {
    if let Some(raw) = configured_value {
        return matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        );
    }

    app_env.eq_ignore_ascii_case("development") || app_env.eq_ignore_ascii_case("local")
}

pub fn extract_ws_token(
    headers: &HeaderMap,
    query: &HashMap<String, String>,
    allow_query_token: bool,
) -> Option<String> {
    if let Some(token) = extract_subprotocol_token(headers) {
        return Some(token);
    }

    if allow_query_token {
        if let Some(token) = query.get("access_token").or_else(|| query.get("token")) {
            let trimmed = token.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    None
}

pub fn extract_subprotocol_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get("sec-websocket-protocol")?.to_str().ok()?;
    for piece in raw.split(',').map(|item| item.trim()) {
        if let Some(encoded) = piece.strip_prefix("authb64.") {
            if let Ok(bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(encoded) {
                if let Ok(token) = String::from_utf8(bytes) {
                    return Some(token);
                }
            }
        }
    }

    None
}

pub fn select_ws_subprotocol(headers: &HeaderMap, preferred: &str) -> Option<String> {
    let raw = headers.get("sec-websocket-protocol")?.to_str().ok()?;

    for piece in raw.split(',').map(|item| item.trim()) {
        if piece == preferred {
            return Some(preferred.to_string());
        }
    }

    None
}
