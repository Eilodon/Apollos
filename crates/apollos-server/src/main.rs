use apollos_server::{build_router, config::ServerConfig, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "apollos_server=info,tower_http=info".into()),
        )
        .compact()
        .init();

    let config = ServerConfig::from_env();
    config.validate_runtime_requirements();
    let bind_addr = config.bind_addr();

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    let app = build_router(AppState::default());

    tracing::info!(%bind_addr, "apollos server listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut stream) = signal(SignalKind::terminate()) {
            let _ = stream.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
