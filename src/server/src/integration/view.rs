//! The room list a sidecar renders its panels from.

use super::store::StoreData;
use crate::types::{Client, ClientKind, Room};
use std::collections::HashMap;

fn member_kind(c: Option<&Client>) -> &'static str {
    match c {
        Some(c) if c.kind == ClientKind::Bridge => "jellyfin",
        Some(c) if c.bridge_device.is_some() => "plugin_bridge",
        _ => "web",
    }
}

/// Chat rooms of `provider`, oldest first. `owners` maps bridged members
/// (client ids) to the Jellyfin user their device belongs to.
pub fn rooms_json(
    provider: &str,
    rooms: &HashMap<String, Room>,
    clients: &HashMap<String, Client>,
    owners: &HashMap<String, String>,
    store: &StoreData,
) -> Vec<serde_json::Value> {
    let external = |user_id: &str| {
        store
            .link_of(user_id, provider)
            .map(|l| l.external_id.clone())
    };
    let mut list: Vec<&Room> = rooms
        .values()
        .filter(|r| r.chat.as_ref().is_some_and(|c| c.provider == provider))
        .collect();
    list.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then(a.room_id.cmp(&b.room_id))
    });
    list.into_iter()
        .map(|r| {
            let chat = r.chat.as_ref().expect("filtered above");
            let members: Vec<_> = r
                .clients
                .iter()
                .map(|id| {
                    let c = clients.get(id);
                    let owner = owners.get(id);
                    serde_json::json!({
                        "id": id,
                        "name": c.map(|c| c.user_name.as_str()).unwrap_or("Someone"),
                        "kind": member_kind(c),
                        "is_host": r.host_id == *id,
                        "status": r.client_status.get(id).map(String::as_str).unwrap_or("unknown"),
                        "owner_user_id": owner,
                        "owner_external_id": owner.and_then(|o| external(o)),
                    })
                })
                .collect();
            let host = (!r.is_hostless()).then(|| {
                serde_json::json!({
                    "id": r.host_id,
                    "name": clients.get(&r.host_id).map(|c| c.user_name.as_str()).unwrap_or("Someone"),
                })
            });
            serde_json::json!({
                "id": r.room_id,
                "name": r.name,
                "has_password": r.password_hash.is_some(),
                "owner": {
                    "user_id": chat.owner,
                    "name": chat.owner_name,
                    "external_id": external(&chat.owner),
                },
                "participants": chat.participants.iter().map(|p| serde_json::json!({
                    "user_id": p.user_id,
                    "name": p.name,
                    "external_id": external(&p.user_id),
                })).collect::<Vec<_>>(),
                "host": host,
                "members": members,
                "media_id": r.media_id,
                "play_state": r.state.play_state,
                "panel": chat.panel.as_ref().map(|p| serde_json::json!({
                    "channel_id": p.channel_id,
                    "message_id": p.message_id,
                })),
                "created_at": r.created_at,
                "empty_since": chat.empty_since,
            })
        })
        .collect()
}
