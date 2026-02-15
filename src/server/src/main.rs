mod auth;
mod messaging;
mod room;
mod routes;
mod tasks;
mod types;
mod utils;
mod ws;

use crate::auth::JwtConfig;
use crate::types::{Clients, Rooms};
use log::info;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use warp::Filter;

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

    let routes =
        routes::build_ws_route(clients, rooms, jwt_config.clone(), allowed_origins.clone())
            .or(routes::build_health_route(jwt_config, allowed_origins));

    let shutdown_rx = tasks::setup_shutdown_signal();

    info!("OpenWatchParty server listening on 0.0.0.0:3000");
    let (_, server) =
        warp::serve(routes).bind_with_graceful_shutdown(([0, 0, 0, 0], 3000), async {
            shutdown_rx.await.ok();
        });

    server.await;
    info!("Server shutdown complete");
}
