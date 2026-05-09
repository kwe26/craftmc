mod auth;
mod backup;
mod config;
mod console;
mod deals;
mod files;
mod marketplace;
mod messages;
mod metrics;
mod players;
mod plugins;
mod region;
mod routes;
mod server;
mod state;
mod tasks;
mod ws;

use std::net::SocketAddr;
use anyhow::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,craftmc=debug".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let app_state = state::AppState::load().await?;
    let app = routes::router(app_state.clone());

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    tracing::info!("CraftMC Manager listening on http://{addr}/control");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}
