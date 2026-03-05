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
}
