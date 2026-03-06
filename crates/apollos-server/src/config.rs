#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub app_env: String,
}

impl ServerConfig {
    pub fn from_env() -> Self {
        let host = std::env::var("APOLLOS_SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = std::env::var("PORT")
            .ok()
            .and_then(|raw| raw.parse::<u16>().ok())
            .unwrap_or(8000);
        let app_env = std::env::var("APP_ENV").unwrap_or_else(|_| "development".to_string());

        Self {
            host,
            port,
            app_env,
        }
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    pub fn validate_runtime_requirements(&self) -> anyhow::Result<()> {
        let production = self.app_env.eq_ignore_ascii_case("production");
        if !production {
            return Ok(());
        }

        let missing_oidc = ["OIDC_ISSUER", "OIDC_AUDIENCE", "OIDC_JWKS_URL"]
            .iter()
            .copied()
            .filter(|required| {
                !std::env::var(required)
                    .ok()
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();

        if !missing_oidc.is_empty() {
            anyhow::bail!(
                "missing required production OIDC env vars: {}",
                missing_oidc.join(", ")
            );
        }

        if !std::env::var("ENABLE_GEMINI_LIVE")
            .ok()
            .map(|value| {
                !matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "0" | "false" | "off" | "no"
                )
            })
            .unwrap_or(true)
        {
            anyhow::bail!("ENABLE_GEMINI_LIVE must be enabled in production");
        }

        let ws_auth_mode =
            std::env::var("WS_AUTH_MODE").unwrap_or_else(|_| "oidc_broker".to_string());
        if !ws_auth_mode.eq_ignore_ascii_case("oidc_broker") {
            anyhow::bail!("WS_AUTH_MODE must be set to oidc_broker in production");
        }

        Ok(())
    }
}
