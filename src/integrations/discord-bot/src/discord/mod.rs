//! The Discord bot: `/jwp` commands, buttons, menus and modals over the
//! gateway, and one live panel message per room.

mod commands;
mod flush;
mod handler;
mod ids;
mod panel;
mod text;

use crate::api::Api;
use crate::core;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use twilight_gateway::{
    CloseFrame, Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt as _,
};
use twilight_http::Client;

/// Runs the Discord bot until `stop` turns true (`Ok`), or until Discord
/// refuses it for good (`Err`, e.g. a bad token).
pub async fn run(token: String, api: Api, mut stop: watch::Receiver<bool>) -> Result<(), String> {
    let http = Arc::new(Client::new(token.clone()));
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
                    return Err("Discord refused DISCORD_BOT_TOKEN".into());
                }
                log::warn!("Discord not reachable yet: {}", e);
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(10)) => {}
            _ = stop.wait_for(|s| *s) => return Ok(()),
        }
    };

    let shared = Arc::new(handler::Shared::new(api, http, app_id));
    let bot = handler::Bot {
        shared: shared.clone(),
    };
    let tasks = [
        tokio::spawn(core::config_loop(shared.clone())),
        tokio::spawn(core::rooms_loop(shared.clone())),
        tokio::spawn(flush::flush_loop(shared.clone())),
    ];

    // Interactions arrive over the gateway whatever the intents; GUILDS is
    // the minimum and needs no privileged intent.
    let mut shard = Shard::new(ShardId::ONE, token, Intents::GUILDS);
    let sender = shard.sender();
    {
        let mut stop = stop.clone();
        tokio::spawn(async move {
            if stop.wait_for(|s| *s).await.is_ok() {
                let _ = sender.close(CloseFrame::NORMAL);
            }
        });
    }
    let stopping = || *stop.borrow();

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
                shared.set_bot_name(&r.user.name);
                let s = shared.clone();
                tokio::spawn(async move { s.sync_commands().await });
            }
            Event::InteractionCreate(i) => {
                let bot = bot.clone();
                tokio::spawn(async move { bot.handle(i.0).await });
            }
            Event::GatewayClose(frame) => {
                if stopping() {
                    break;
                }
                log::warn!("Discord gateway closed: {:?}", frame);
            }
            _ => {}
        }
    }
    for t in tasks {
        t.abort();
    }
    if stopping() {
        Ok(())
    } else {
        Err("Discord connection ended for good (bad token or intents?)".into())
    }
}
