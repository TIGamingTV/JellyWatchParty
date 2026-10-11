//! JellyWatchParty chat bot: a sidecar that lets linked users run watch
//! parties for their own Jellyfin devices from a chat platform. It holds no
//! state and decides nothing: it relays who is asking to the session
//! server's integration API, and keeps each room's panel message up to
//! date. Shared parts live at the top level; each platform in its module.

mod api;
mod config;
mod core;
mod discord;
mod panels;
mod telegram;
mod text;

use tokio::sync::watch;

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
    for w in &cfg.warnings {
        log::warn!("{}", w);
    }
    log::info!("Session server: {}", cfg.api_url);
    let api = |token: &str, var: &'static str| match api::Api::new(&cfg.api_url, token, var) {
        Ok(a) => a,
        Err(e) => {
            log::error!("{}", e);
            std::process::exit(2);
        }
    };

    let (stop_tx, stop) = watch::channel(false);
    tokio::spawn(async move {
        wait_for_signal().await;
        log::info!("Shutting down");
        let _ = stop_tx.send(true);
    });

    let mut platforms = Vec::new();
    if let Some(t) = &cfg.discord {
        let api = api(&t.integration, "DISCORD_INTEGRATION_TOKEN");
        platforms.push((
            "Discord",
            tokio::spawn(discord::run(t.bot.clone(), api, stop.clone())),
        ));
    }
    if let Some(t) = &cfg.telegram {
        let api = api(&t.integration, "TELEGRAM_INTEGRATION_TOKEN");
        platforms.push((
            "Telegram",
            tokio::spawn(telegram::run(
                t.bot.clone(),
                cfg.telegram_api_url.clone(),
                api,
                stop.clone(),
            )),
        ));
    }

    // A platform that fails only stops itself; the bot exits once every
    // platform has stopped.
    let mut failed = false;
    for (name, task) in platforms {
        match task.await {
            Ok(Ok(())) => log::info!("{} stopped", name),
            Ok(Err(e)) => {
                log::error!("{}", e);
                failed = true;
            }
            Err(e) => {
                log::error!("{} crashed: {}", name, e);
                failed = true;
            }
        }
    }
    if failed {
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
