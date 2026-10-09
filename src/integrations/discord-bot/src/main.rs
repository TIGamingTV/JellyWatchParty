//! JellyWatchParty Discord bot: a sidecar that lets linked users run watch
//! parties for their own Jellyfin devices from Discord. It holds no state
//! and decides nothing: it relays who is asking to the session server's
//! integration API, and keeps each room's panel message up to date.

mod api;
mod commands;
mod config;
mod handler;
mod ids;
mod panel;
mod sync;
mod text;

use std::sync::Arc;
use std::time::Duration;
use twilight_gateway::{
    CloseFrame, Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt as _,
};
use twilight_http::Client;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cfg = match config::Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            log::error!("{}", e);
            std::process::exit(2);
        }
    };
    let api = match api::Api::new(&cfg.api_url, &cfg.api_token) {
        Ok(a) => a,
        Err(e) => {
            log::error!("{}", e);
            std::process::exit(2);
        }
    };
    log::info!("Session server: {}", cfg.api_url);

    let http = Arc::new(Client::new(cfg.discord_token.clone()));
    let app_id = loop {
        match http.current_user_application().await {
            Ok(res) => match res.model().await {
                Ok(app) => break app.id,
                Err(e) => log::error!("Discord: unexpected application info: {}", e),
            },
            Err(e) => {
                use twilight_http::error::ErrorType;
                let refused = match e.kind() {
                    ErrorType::Unauthorized => true,
                    ErrorType::Response { status, .. } => status.get() == 401,
                    _ => false,
                };
                if refused {
                    log::error!("Discord refused DISCORD_BOT_TOKEN");
                    std::process::exit(1);
                }
                log::warn!("Discord not reachable yet: {}", e);
            }
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    };

    let shared = Arc::new(handler::Shared::new(api, http, app_id));
    let bot = handler::Bot {
        shared: shared.clone(),
    };
    tokio::spawn(sync::config_loop(shared.clone()));
    tokio::spawn(sync::rooms_loop(shared.clone()));
    tokio::spawn(sync::flush_loop(shared.clone()));

    // Interactions arrive over the gateway whatever the intents; GUILDS is
    // the minimum and needs no privileged intent.
    let mut shard = Shard::new(ShardId::ONE, cfg.discord_token, Intents::GUILDS);
    let sender = shard.sender();
    let stopping = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let stopping = stopping.clone();
        tokio::spawn(async move {
            wait_for_signal().await;
            log::info!("Shutting down");
            stopping.store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = sender.close(CloseFrame::NORMAL);
        });
    }

    // Gateway close events always come through (they have no flag).
    let wanted = EventTypeFlags::READY | EventTypeFlags::INTERACTION_CREATE;
    while let Some(item) = shard.next_event(wanted).await {
        let event = match item {
            Ok(e) => e,
            Err(e) => {
                log::warn!("Discord gateway: {}", e);
                continue;
            }
        };
        match event {
            Event::Ready(r) => {
                log::info!("Connected to Discord as {}", r.user.name);
                *shared.bot_name.write().unwrap_or_else(|e| e.into_inner()) = r.user.name.clone();
                let s = shared.clone();
                tokio::spawn(async move { s.sync_commands().await });
            }
            Event::InteractionCreate(i) => {
                let bot = bot.clone();
                tokio::spawn(async move { bot.handle(i.0).await });
            }
            Event::GatewayClose(frame) => {
                if stopping.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                log::warn!("Discord gateway closed: {:?}", frame);
            }
            _ => {}
        }
    }
    if !stopping.load(std::sync::atomic::Ordering::SeqCst) {
        log::error!("Discord connection ended for good (bad token or intents?)");
        std::process::exit(1);
    }
}

async fn wait_for_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
        let mut int = signal(SignalKind::interrupt()).expect("SIGINT handler");
        tokio::select! {
            _ = term.recv() => {}
            _ = int.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
