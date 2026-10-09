mod admin;
mod auth;
mod integration;
mod jellyfin;
mod messaging;
mod password;
mod room;
mod routes;
mod tasks;
mod types;
mod utils;
mod ws;

#[cfg(test)]
mod test_helpers;

use crate::admin::config::{AdminSetup, RECOMMENDED_MIN_PASSWORD_LEN};
use crate::auth::JwtConfig;
use crate::jellyfin::JellyfinSetup;
use crate::types::{Clients, Rooms};
use log::{info, warn};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let jwt_config = Arc::new(JwtConfig::from_env());
    let allowed_origins = Arc::new(routes::get_allowed_origins());

    info!("Allowed origins: {:?}", allowed_origins);
    info!(
        "JWT: {}",
        if jwt_config.enabled {
            "ENABLED"
        } else {
            "DISABLED"
        }
    );

    let clients: Clients = Arc::new(RwLock::new(HashMap::new()));
    let rooms: Rooms = Arc::new(RwLock::new(HashMap::new()));

    tasks::spawn_zombie_cleanup(clients.clone(), rooms.clone());

    let shutdown_rx = tasks::setup_shutdown_signal();

    start_admin_panel(&clients, &rooms, jwt_config.enabled, shutdown_rx.clone());

    let app = routes::build_router(clients, rooms, jwt_config, allowed_origins);

    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let addr: SocketAddr = format!("{}:{}", host, port)
        .parse()
        .expect("Invalid HOST:PORT combination");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("Failed to bind {}: {}", addr, e));

    info!("JellyWatchParty server listening on {}", addr);
    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(tasks::wait_for_shutdown(shutdown_rx))
        .await
    {
        log::error!("Server error: {}", e);
    }

    info!("Server shutdown complete");
}

/// Starts the admin panel on its own port unless it's switched off or has
/// no password configured. Problems here never stop the main server.
fn start_admin_panel(
    clients: &Clients,
    rooms: &Rooms,
    jwt_enabled: bool,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    match AdminSetup::from_env() {
        AdminSetup::Disabled => info!("Admin panel: disabled (ADMIN_ENABLED=false)"),
        AdminSetup::Misconfigured(reason) => {
            warn!("Admin panel: NOT started - {}", reason);
        }
        AdminSetup::Enabled(cfg) => {
            if cfg.password.chars().count() < RECOMMENDED_MIN_PASSWORD_LEN {
                warn!(
                    "SECURITY: ADMIN_PASSWORD is shorter than {} characters; use a long random one",
                    RECOMMENDED_MIN_PASSWORD_LEN
                );
            }
            tasks::spawn_empty_group_reaper(clients.clone(), rooms.clone(), cfg.empty_group_ttl_ms);
            let jellyfin = start_jellyfin_bridge(clients, rooms);
            let integration = integration::start(
                match &jellyfin {
                    admin::JellyfinStatus::Enabled(b) => Some(b),
                    admin::JellyfinStatus::Unavailable(_) => None,
                },
                tasks::wait_for_shutdown(shutdown_rx.clone()),
            );
            let state = admin::AdminState::new(
                clients.clone(),
                rooms.clone(),
                cfg,
                jwt_enabled,
                jellyfin,
                integration,
            );
            tokio::spawn(admin::serve(state, tasks::wait_for_shutdown(shutdown_rx)));
        }
    }
}

/// The Jellyfin device bridge belongs to the admin panel: it is only set up
/// when the panel runs, and only with `JELLYFIN_URL` + `JELLYFIN_API_KEY`.
fn start_jellyfin_bridge(clients: &Clients, rooms: &Rooms) -> admin::JellyfinStatus {
    let reason = match JellyfinSetup::from_env() {
        JellyfinSetup::Enabled(cfg) => {
            match jellyfin::Bridges::start(&cfg, clients.clone(), rooms.clone()) {
                Ok(b) => {
                    info!(
                        "Admin panel: Jellyfin devices enabled ({}, polling every {} ms)",
                        cfg.url, cfg.poll_interval_ms
                    );
                    return admin::JellyfinStatus::Enabled(b);
                }
                Err(e) => e,
            }
        }
        JellyfinSetup::NotConfigured => {
            info!("Admin panel: Jellyfin devices off (set JELLYFIN_URL and JELLYFIN_API_KEY to bridge TV apps and other clients)");
            return admin::JellyfinStatus::Unavailable(
                "Set JELLYFIN_URL and JELLYFIN_API_KEY on the session server to add Jellyfin devices"
                    .into(),
            );
        }
        JellyfinSetup::Misconfigured(e) => e,
    };
    warn!("Admin panel: Jellyfin devices off - {}", reason);
    admin::JellyfinStatus::Unavailable(reason)
}
