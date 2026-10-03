//! Admin JSON API. Every handler except `login` sits behind
//! `require_session`. Mutations take the room/client locks in the usual
//! rooms -> clients order, log an `admin:` audit line, and refresh every
//! lobby's room list afterwards.

use super::{auth, error_response, AdminState, ClientIp};
use crate::messaging::broadcast_room_list;
use crate::room::ops::{self, AddOptions, OpError};
use crate::types::{Client, Room};
use crate::utils::now_ms;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use log::{info, warn};
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;

/// An API failure: status plus a message for the UI. Kept small so
/// `Result<Response, ApiError>` stays cheap to return.
pub struct ApiError(StatusCode, &'static str);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        error_response(self.0, self.1)
    }
}

type ApiResult = Result<Response, ApiError>;

fn op_error(e: OpError) -> ApiError {
    let status = match e {
        OpError::RoomNotFound | OpError::ClientNotFound | OpError::NotAMember => {
            StatusCode::NOT_FOUND
        }
        OpError::RoomFull => StatusCode::CONFLICT,
        OpError::InvalidName => StatusCode::BAD_REQUEST,
    };
    ApiError(status, e.message())
}

fn ok() -> Response {
    Json(serde_json::json!({ "ok": true })).into_response()
}

// --- login -----------------------------------------------------------------

#[derive(Deserialize)]
pub struct LoginBody {
    username: String,
    password: String,
}

pub async fn login(
    State(state): State<AdminState>,
    ClientIp(ip): ClientIp,
    Json(body): Json<LoginBody>,
) -> Response {
    let now = now_ms();
    let mut store = state.auth();
    if let Some(wait) = store.login_blocked(ip, now) {
        warn!(
            "admin: login from {} refused (too many failed attempts)",
            ip
        );
        let mut res = error_response(
            StatusCode::TOO_MANY_REQUESTS,
            &format!(
                "Too many failed logins. Try again in {}s",
                wait.div_ceil(1000)
            ),
        );
        if let Ok(v) = (wait.div_ceil(1000)).to_string().parse() {
            res.headers_mut().insert(header::RETRY_AFTER, v);
        }
        return res;
    }
    if !auth::credentials_match(&state.cfg, &body.username, &body.password) {
        store.record_failure(ip, now);
        warn!("admin: failed login from {}", ip);
        return error_response(StatusCode::UNAUTHORIZED, "Wrong username or password");
    }
    store.clear_failures(ip);
    let token = store.create_session(now, state.cfg.session_ttl_ms);
    drop(store);
    info!("admin: login from {}", ip);
    let cookie = auth::session_cookie(&token, state.cfg.session_ttl_ms, state.cfg.cookie_secure);
    (
        [(header::SET_COOKIE, cookie)],
        Json(serde_json::json!({ "username": state.cfg.username })),
    )
        .into_response()
}

pub async fn logout(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    if let Some(t) = auth::cookie_token(&headers) {
        state.auth().revoke(&t);
    }
    info!("admin: logout");
    (
        [(
            header::SET_COOKIE,
            auth::clear_cookie(state.cfg.cookie_secure),
        )],
        ok(),
    )
        .into_response()
}

pub async fn me(State(state): State<AdminState>) -> Response {
    Json(serde_json::json!({ "username": state.cfg.username })).into_response()
}

// --- overview --------------------------------------------------------------

fn expected_position(room: &Room, now: u64) -> f64 {
    if room.state.play_state == "playing" {
        room.state.position + now.saturating_sub(room.last_state_ts) as f64 / 1000.0
    } else {
        room.state.position
    }
}

fn member_json(id: &str, room: &Room, clients: &HashMap<String, Client>) -> serde_json::Value {
    let client = clients.get(id);
    serde_json::json!({
        "id": id,
        "name": client.map(|c| c.user_name.as_str()).unwrap_or("Someone"),
        "kind": "web",
        "is_host": room.host_id == id,
        "status": room.client_status.get(id).map(String::as_str).unwrap_or("unknown"),
        "ready": room.ready_clients.contains(id),
        "connected": client.is_some_and(|c| !c.sender.is_closed()),
    })
}

/// Everything the dashboard shows, in one snapshot.
pub fn build_overview(
    rooms: &HashMap<String, Room>,
    clients: &HashMap<String, Client>,
    now: u64,
) -> serde_json::Value {
    let mut room_list: Vec<&Room> = rooms.values().collect();
    room_list.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.name.cmp(&b.name)));
    let rooms_json: Vec<_> = room_list
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.room_id,
                "name": r.name,
                "admin_created": r.admin_created,
                "has_password": r.password_hash.is_some(),
                "media_id": r.media_id,
                "host_id": if r.is_hostless() { None } else { Some(&r.host_id) },
                "position": expected_position(r, now),
                "play_state": r.state.play_state,
                "started": r.started,
                "pending_play": r.pending_play.is_some(),
                "created_at": r.created_at,
                "members": r.clients.iter().map(|id| member_json(id, r, clients)).collect::<Vec<_>>(),
            })
        })
        .collect();

    let mut unassigned: Vec<_> = clients
        .iter()
        .filter(|(_, c)| c.room_id.is_none())
        .map(|(id, c)| {
            serde_json::json!({
                "id": id,
                "name": if c.user_name.is_empty() { "(not signed in)" } else { &c.user_name },
                "kind": "web",
                "authenticated": c.authenticated,
                "connected": !c.sender.is_closed(),
                "connected_at": c.connected_at,
            })
        })
        .collect();
    unassigned.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));

    serde_json::json!({
        "now": now,
        "rooms": rooms_json,
        "unassigned": unassigned,
        "totals": {
            "rooms": rooms.len(),
            "clients": clients.len(),
        },
    })
}

pub async fn overview(State(state): State<AdminState>) -> Response {
    let mut body = {
        let locked_rooms = state.rooms.read().await;
        let locked_clients = state.clients.read().await;
        build_overview(&locked_rooms, &locked_clients, now_ms())
    };
    body["server"] = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": now_ms().saturating_sub(state.started_at) / 1000,
        "auth_enabled": state.jwt_enabled,
    });
    Json(body).into_response()
}

// --- rooms -----------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateRoomBody {
    name: String,
    #[serde(default)]
    password: Option<String>,
}

pub async fn create_room(
    State(state): State<AdminState>,
    Json(body): Json<CreateRoomBody>,
) -> ApiResult {
    let id = {
        let mut locked_rooms = state.rooms.write().await;
        ops::create_group(&body.name, body.password.as_deref(), &mut locked_rooms)
            .map_err(op_error)?
    };
    info!("admin: created group {}", id);
    broadcast_room_list(&state.clients, &state.rooms).await;
    Ok((StatusCode::CREATED, Json(serde_json::json!({ "id": id }))).into_response())
}

/// Distinguishes a missing field (`None`) from an explicit `null`
/// (`Some(None)`).
fn double_option<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}

#[derive(Deserialize)]
pub struct UpdateRoomBody {
    #[serde(default)]
    name: Option<String>,
    /// Absent: keep. `null` or `""`: remove. Anything else: set.
    #[serde(default, deserialize_with = "double_option")]
    password: Option<Option<String>>,
}

pub async fn update_room(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateRoomBody>,
) -> ApiResult {
    {
        let mut locked_rooms = state.rooms.write().await;
        let pw = body.password.as_ref().map(|p| p.as_deref());
        ops::update_room(&id, body.name.as_deref(), pw, &mut locked_rooms).map_err(op_error)?;
    }
    info!(
        "admin: updated room {} (name: {}, password: {})",
        id,
        body.name.is_some(),
        match &body.password {
            None => "kept",
            Some(None) => "removed",
            Some(Some(p)) if p.is_empty() => "removed",
            Some(Some(_)) => "changed",
        }
    );
    broadcast_room_list(&state.clients, &state.rooms).await;
    Ok(ok())
}

pub async fn delete_room(State(state): State<AdminState>, Path(id): Path<String>) -> ApiResult {
    {
        let mut locked_rooms = state.rooms.write().await;
        let mut locked_clients = state.clients.write().await;
        ops::close_room(
            &id,
            "Closed by an admin",
            &mut locked_rooms,
            &mut locked_clients,
        )
        .map_err(op_error)?;
    }
    info!("admin: closed room {}", id);
    broadcast_room_list(&state.clients, &state.rooms).await;
    Ok(ok())
}

// --- members ---------------------------------------------------------------

#[derive(Deserialize)]
pub struct AddMemberBody {
    client_id: String,
}

pub async fn add_member(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(body): Json<AddMemberBody>,
) -> ApiResult {
    {
        let mut locked_rooms = state.rooms.write().await;
        let mut locked_clients = state.clients.write().await;
        let client = locked_clients
            .get(&body.client_id)
            .ok_or_else(|| op_error(OpError::ClientNotFound))?;
        if !client.authenticated {
            // Adding it would let a connection that never proved who it is
            // skip authentication entirely.
            return Err(ApiError(
                StatusCode::CONFLICT,
                "This client has not signed in yet",
            ));
        }
        let opts = AddOptions {
            by_admin: true,
            promote_if_hostless: true,
        };
        ops::add_member(
            &id,
            &body.client_id,
            &mut locked_rooms,
            &mut locked_clients,
            opts,
        )
        .map_err(op_error)?;
    }
    info!("admin: added client {} to room {}", body.client_id, id);
    broadcast_room_list(&state.clients, &state.rooms).await;
    Ok(ok())
}

pub async fn remove_member(
    State(state): State<AdminState>,
    Path((id, member)): Path<(String, String)>,
) -> ApiResult {
    {
        let mut locked_rooms = state.rooms.write().await;
        let mut locked_clients = state.clients.write().await;
        ops::kick_member(
            &id,
            &member,
            "An admin removed you from the room",
            &mut locked_rooms,
            &mut locked_clients,
        )
        .map_err(op_error)?;
    }
    info!("admin: removed client {} from room {}", member, id);
    broadcast_room_list(&state.clients, &state.rooms).await;
    Ok(ok())
}

#[derive(Deserialize)]
pub struct SetHostBody {
    member: String,
}

pub async fn set_host(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(body): Json<SetHostBody>,
) -> ApiResult {
    {
        let mut locked_rooms = state.rooms.write().await;
        let locked_clients = state.clients.read().await;
        ops::set_host(&id, &body.member, &mut locked_rooms, &locked_clients).map_err(op_error)?;
    }
    info!("admin: made {} host of room {}", body.member, id);
    Ok(ok())
}
