mod api;
mod application;
mod engine;

use std::net::SocketAddr;
use tracing::info;

#[tokio::main]
async fn main() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,ai_request=info"));

    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    let bind = std::env::var("AI_ENGINE_BIND").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let addr: SocketAddr = bind.parse().expect("invalid AI_ENGINE_BIND");

    info!("shogi-ai listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind");

    axum::serve(listener, api::router())
        .await
        .expect("server error");
}
