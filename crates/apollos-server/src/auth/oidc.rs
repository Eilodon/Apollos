use std::str::FromStr;
use std::time::Duration;

use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use once_cell::sync::Lazy;
use serde_json::Value;
use tokio::sync::RwLock;
use tracing::warn;

const JWKS_CACHE_TTL_SECONDS: f64 = 300.0;

#[derive(Debug, Clone)]
pub struct OidcIdentity {
    pub subject: String,
}

#[derive(Debug, Clone)]
struct OidcRuntimeConfig {
    issuer: Option<String>,
    audience: Option<String>,
    jwks_url: Option<String>,
    algorithms: Vec<Algorithm>,
    leeway_seconds: u64,
    strict_mode: bool,
    allow_insecure_dev_tokens: bool,
}

#[derive(Debug, Clone)]
struct CachedJwks {
    jwks_url: String,
    fetched_epoch: f64,
    keyset: JwkSet,
}

static JWKS_CACHE: Lazy<RwLock<Option<CachedJwks>>> = Lazy::new(|| RwLock::new(None));
static OIDC_HTTP: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
});

pub async fn verify_id_token(token: &str) -> Option<OidcIdentity> {
    let cfg = OidcRuntimeConfig::from_env();
    let has_oidc_cfg = cfg.issuer.is_some() && cfg.audience.is_some() && cfg.jwks_url.is_some();

    if has_oidc_cfg {
        return verify_with_jwks(token, &cfg).await;
    }

    if cfg.strict_mode {
        warn!("strict OIDC mode is enabled but OIDC_ISSUER/OIDC_AUDIENCE/OIDC_JWKS_URL is missing");
        return None;
    }

    if cfg.allow_insecure_dev_tokens {
        return verify_insecure_dev_token(token);
    }

    None
}

impl OidcRuntimeConfig {
    fn from_env() -> Self {
        let app_env = std::env::var("APP_ENV").unwrap_or_else(|_| "development".to_string());
        let strict_mode =
            app_env.eq_ignore_ascii_case("production") || env_flag("OIDC_REQUIRE_STRICT", false);

        let allow_insecure_dev_tokens = env_flag("OIDC_ALLOW_INSECURE_DEV_TOKENS", !strict_mode);

        let issuer = read_non_empty_env("OIDC_ISSUER");
        let audience = read_non_empty_env("OIDC_AUDIENCE");
        let jwks_url = read_non_empty_env("OIDC_JWKS_URL");

        let algorithms_raw =
            std::env::var("OIDC_ALGORITHMS").unwrap_or_else(|_| "RS256,ES256".to_string());
        let mut algorithms = algorithms_raw
            .split(',')
            .filter_map(|entry| {
                let value = entry.trim();
                if value.is_empty() {
                    return None;
                }
                Algorithm::from_str(value).ok()
            })
            .collect::<Vec<_>>();
        if algorithms.is_empty() {
            algorithms = vec![Algorithm::RS256, Algorithm::ES256];
        }

        let leeway_seconds = std::env::var("OIDC_LEEWAY_SECONDS")
            .ok()
            .and_then(|raw| raw.parse::<u64>().ok())
            .unwrap_or(30)
            .min(300);

        Self {
            issuer,
            audience,
            jwks_url,
            algorithms,
            leeway_seconds,
            strict_mode,
            allow_insecure_dev_tokens,
        }
    }
}

fn read_non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

async fn verify_with_jwks(token: &str, cfg: &OidcRuntimeConfig) -> Option<OidcIdentity> {
    let issuer = cfg.issuer.as_ref()?;
    let audience = cfg.audience.as_ref()?;
    let jwks_url = cfg.jwks_url.as_ref()?;

    let header = decode_header(token).ok()?;
    if !cfg.algorithms.contains(&header.alg) {
        return None;
    }

    let cached = get_jwks_cached(jwks_url, false).await?;
    if let Some(identity) = verify_against_keyset(token, &header, issuer, audience, cfg, &cached) {
        return Some(identity);
    }

    if header.kid.is_some() {
        let refreshed = get_jwks_cached(jwks_url, true).await?;
        return verify_against_keyset(token, &header, issuer, audience, cfg, &refreshed);
    }

    None
}

fn verify_against_keyset(
    token: &str,
    header: &jsonwebtoken::Header,
    issuer: &str,
    audience: &str,
    cfg: &OidcRuntimeConfig,
    keyset: &JwkSet,
) -> Option<OidcIdentity> {
    let candidates = if let Some(kid) = header.kid.as_deref() {
        keyset
            .find(kid)
            .map(|value| vec![value])
            .unwrap_or_default()
    } else {
        keyset.keys.iter().collect::<Vec<_>>()
    };

    let mut validation = Validation::new(header.alg);
    validation.algorithms = cfg.algorithms.clone();
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[audience]);
    validation.leeway = cfg.leeway_seconds;

    for jwk in candidates {
        if let Some(key_alg) = jwk.common.key_algorithm {
            if let Ok(parsed_alg) = Algorithm::from_str(&key_alg.to_string()) {
                if parsed_alg != header.alg {
                    continue;
                }
            }
        }

        let Ok(decoding_key) = DecodingKey::from_jwk(jwk) else {
            continue;
        };

        let Ok(decoded) = decode::<Value>(token, &decoding_key, &validation) else {
            continue;
        };

        let subject = decoded
            .claims
            .get("sub")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())?;

        return Some(OidcIdentity {
            subject: format!("oidc:{subject}"),
        });
    }

    None
}

async fn get_jwks_cached(jwks_url: &str, force_refresh: bool) -> Option<JwkSet> {
    let now = now_epoch();

    if !force_refresh {
        let guard = JWKS_CACHE.read().await;
        if let Some(cache) = guard.as_ref() {
            if cache.jwks_url == jwks_url && (now - cache.fetched_epoch) < JWKS_CACHE_TTL_SECONDS {
                return Some(cache.keyset.clone());
            }
        }
    }

    let keyset = fetch_jwks(jwks_url).await?;
    {
        let mut guard = JWKS_CACHE.write().await;
        *guard = Some(CachedJwks {
            jwks_url: jwks_url.to_string(),
            fetched_epoch: now,
            keyset: keyset.clone(),
        });
    }
    Some(keyset)
}

async fn fetch_jwks(jwks_url: &str) -> Option<JwkSet> {
    let response = OIDC_HTTP.get(jwks_url).send().await.ok()?;
    if !response.status().is_success() {
        warn!(status = %response.status(), "failed to fetch OIDC JWKS");
        return None;
    }
    response.json::<JwkSet>().await.ok()
}

fn verify_insecure_dev_token(token: &str) -> Option<OidcIdentity> {
    let trimmed = token.trim();
    if trimmed.len() < 16 {
        return None;
    }

    Some(OidcIdentity {
        subject: format!("oidc:{}", &trimmed[..12]),
    })
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

fn now_epoch() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}
