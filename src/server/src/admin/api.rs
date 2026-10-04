//! Admin JSON API. Every handler except `login` sits behind
//! `require_session`. Mutations take the room/client locks in the usual
//! rooms -> clients order, log an `admin:` audit line, and refresh every
//! lobby's room list afterwards.

use super::{auth, error_response, AdminState, ClientIp, JellyfinStatus};
use crate::jellyfin::api::{JfSession, LISTED_ACTIVE_WITHIN_MS};
use crate::jellyfin::bridge::{AddError, Role, Snapshot};
use crate::jellyfin::logic::{device_view, MAX_EXTRAPOLATION_SECS};
use crate::jellyfin::time::parse_utc_ms;
use crate::jellyfin::Bridges;
use crate::messaging::broadcast_room_list;
use crate::room::ops::{self, AddOptions, OpError};
use crate::types::{Client, ClientKind, Room};
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
pub struct ApiError(StatusCode, std::borrow::Cow<'static, str>);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        error_response(self.0, &self.1)
    }
}

fn bridge_error(e: AddError) -> ApiError {
    let status = match &e {
        AddError::Unavailable(_) => StatusCode::BAD_GATEWAY,
        AddError::SessionNotFound => StatusCode::NOT_FOUND,
        AddError::Op(op) => return op_error(*op),
        _ => StatusCode::CONFLICT,
    };
    ApiError(status, e.message().into())
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
    ApiError(status, e.message().into())
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

fn member_json(
    id: &str,
    room: &Room,
    clients: &HashMap<String, Client>,
    bridges: Option<&Bridges>,
) -> serde_json::Value {
    let client = clients.get(id);
    let mut m = serde_json::json!({
        "id": id,
        "name": client.map(|c| c.user_name.as_str()).unwrap_or("Someone"),
        "kind": "web",
        "is_host": room.host_id == id,
        "status": room.client_status.get(id).map(String::as_str).unwrap_or("unknown"),
        "ready": room.ready_clients.contains(id),
        "connected": client.is_some_and(|c| c.connected),
    });
    if client.is_some_and(|c| c.kind == ClientKind::Bridge) {
        m["kind"] = "jellyfin".into();
        if let Some(info) = bridges.and_then(|b| b.info(id)) {
            m["status"] = info.status.into();
            m["connected"] = (info.status != "offline").into();
            m["drift"] = info.drift.map(|d| (d * 10.0).round() / 10.0).into();
            m["detail"] = info.detail.into();
            m["device"] = if info.client_name.is_empty() {
                info.device_name.into()
            } else {
                format!("{} - {}", info.device_name, info.client_name).into()
            };
            m["remote_control"] = info.remote_control.into();
        }
    }
    m
}

/// A readable name for an item id, if some Jellyfin session is playing it.
fn media_name(snapshot: Option<&Snapshot>, media_id: Option<&str>) -> Option<String> {
    let id = media_id?;
    snapshot?
        .sessions
        .iter()
        .find(|s| s.item_id().as_deref() == Some(id))
        .and_then(|s| s.item_name())
}

/// Everything the dashboard shows, in one snapshot.
pub fn build_overview(
    rooms: &HashMap<String, Room>,
    clients: &HashMap<String, Client>,
    bridges: Option<&Bridges>,
    now: u64,
) -> serde_json::Value {
    let snapshot = bridges.map(|b| b.snapshot());
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
                "media_name": media_name(snapshot.as_deref(), r.media_id.as_deref()),
                "host_id": if r.is_hostless() { None } else { Some(&r.host_id) },
                "position": expected_position(r, now),
                "play_state": r.state.play_state,
                "started": r.started,
                "pending_play": r.pending_play.is_some(),
                "created_at": r.created_at,
                "members": r.clients.iter().map(|id| member_json(id, r, clients, bridges)).collect::<Vec<_>>(),
            })
        })
        .collect();

    let mut unassigned: Vec<_> = clients
        .iter()
        .filter(|(_, c)| c.room_id.is_none() && c.kind == ClientKind::Web)
        .map(|(id, c)| {
            serde_json::json!({
                "id": id,
                "name": if c.user_name.is_empty() { "(not signed in)" } else { &c.user_name },
                "kind": "web",
                "authenticated": c.authenticated,
                "connected": c.connected,
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
            "clients": clients.values().filter(|c| c.kind == ClientKind::Web).count(),
        },
    })
}

fn jellyfin_json(state: &AdminState) -> serde_json::Value {
    match &state.jellyfin {
        JellyfinStatus::Unavailable(reason) => {
            serde_json::json!({ "enabled": false, "reason": reason })
        }
        JellyfinStatus::Enabled(b) => {
            let snap = b.snapshot();
            serde_json::json!({
                "enabled": true,
                "error": snap.error,
                "last_poll": snap.fetched_at,
            })
        }
    }
}

pub async fn overview(State(state): State<AdminState>) -> Response {
    let mut body = {
        let locked_rooms = state.rooms.read().await;
        let locked_clients = state.clients.read().await;
        build_overview(&locked_rooms, &locked_clients, state.bridges(), now_ms())
    };
    body["server"] = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": now_ms().saturating_sub(state.started_at) / 1000,
        "auth_enabled": state.jwt_enabled,
        "jellyfin": jellyfin_json(&state),
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
    /// A connected client (web client or an already bridged device).
    #[serde(default)]
    client_id: Option<String>,
    /// Or a Jellyfin session to bridge in...
    #[serde(default)]
    jellyfin_session_id: Option<String>,
    /// ...as `host` or `receiver` (default).
    #[serde(default)]
    role: Option<String>,
}

pub async fn add_member(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(body): Json<AddMemberBody>,
) -> ApiResult {
    if let Some(session_id) = body.jellyfin_session_id.as_deref() {
        let Some(bridges) = state.bridges() else {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "Jellyfin devices are not set up (JELLYFIN_URL / JELLYFIN_API_KEY)".into(),
            ));
        };
        let role = match body.role.as_deref() {
            Some("host") => Role::Host,
            None | Some("receiver") => Role::Receiver,
            Some(_) => {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "role must be host or receiver".into(),
                ))
            }
        };
        let client_id = bridges
            .add(&id, session_id, role)
            .await
            .map_err(bridge_error)?;
        info!(
            "admin: bridged Jellyfin session {} into room {} as {:?} (client {})",
            session_id, id, role, client_id
        );
        return Ok(Json(serde_json::json!({ "ok": true, "client_id": client_id })).into_response());
    }

    let Some(client_id) = body.client_id else {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "client_id or jellyfin_session_id is required".into(),
        ));
    };
    {
        let mut locked_rooms = state.rooms.write().await;
        let mut locked_clients = state.clients.write().await;
        let client = locked_clients
            .get(&client_id)
            .ok_or_else(|| op_error(OpError::ClientNotFound))?;
        if !client.authenticated {
            // Adding it would let a connection that never proved who it is
            // skip authentication entirely.
            return Err(ApiError(
                StatusCode::CONFLICT,
                "This client has not signed in yet".into(),
            ));
        }
        let opts = AddOptions {
            by_admin: true,
            promote_if_hostless: true,
        };
        ops::add_member(
            &id,
            &client_id,
            &mut locked_rooms,
            &mut locked_clients,
            opts,
        )
        .map_err(op_error)?;
    }
    info!("admin: added client {} to room {}", client_id, id);
    broadcast_room_list(&state.clients, &state.rooms).await;
    Ok(ok())
}

pub async fn remove_member(
    State(state): State<AdminState>,
    Path((id, member)): Path<(String, String)>,
) -> ApiResult {
    let is_bridge_here = {
        let locked_rooms = state.rooms.read().await;
        let locked_clients = state.clients.read().await;
        let in_room = locked_rooms
            .get(&id)
            .ok_or_else(|| op_error(OpError::RoomNotFound))?
            .clients
            .contains(&member);
        in_room
            && locked_clients
                .get(&member)
                .is_some_and(|c| c.kind == ClientKind::Bridge)
    };
    if is_bridge_here {
        if let Some(b) = state.bridges() {
            if b.remove(&member).await {
                info!("admin: removed bridged device {} from room {}", member, id);
                return Ok(ok());
            }
        }
    }
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

// --- Jellyfin devices ------------------------------------------------------

/// Active within `LISTED_ACTIVE_WITHIN_MS`. A session without a usable
/// `LastActivityDate` is listed rather than hidden.
pub(super) fn recently_active(s: &JfSession, clock_offset_ms: i64, now: u64) -> bool {
    match s.last_activity_date.as_deref().and_then(parse_utc_ms) {
        None => true,
        Some(t) => {
            let local = (t as i64).saturating_add(clock_offset_ms).max(0) as u64;
            now.saturating_sub(local) < LISTED_ACTIVE_WITHIN_MS
        }
    }
}

/// Jellyfin sessions an admin can put into a room: everything active except
/// clients that run the Watch Party panel themselves (they join as web
/// clients) and this server's own API session.
pub async fn jellyfin_sessions(State(state): State<AdminState>) -> Response {
    let bridges = match &state.jellyfin {
        JellyfinStatus::Unavailable(reason) => {
            return Json(serde_json::json!({
                "enabled": false,
                "reason": reason,
                "sessions": [],
            }))
            .into_response()
        }
        JellyfinStatus::Enabled(b) => b,
    };
    let snap = bridges.snapshot_for_admin().await;
    let now = now_ms();
    let mut sessions: Vec<_> = snap
        .sessions
        .iter()
        .filter(|s| !s.is_own() && !s.runs_web_client())
        .map(|s| (s, bridges.bridged_device(s.device_id(), &s.user_id())))
        // Recently active ones, plus anything already bridged.
        .filter(|(s, bridged)| bridged.is_some() || recently_active(s, snap.clock_offset_ms, now))
        .map(|(s, bridged)| {
            let view = device_view(s, now, snap.clock_offset_ms, MAX_EXTRAPOLATION_SECS);
            serde_json::json!({
                "id": s.id,
                "user_name": s.user_name(),
                "device_name": s.device_name(),
                "client": s.client_name(),
                "remote_control": s.supports_remote_control,
                "now_playing": s.item_id().map(|id| serde_json::json!({
                    "id": id,
                    "name": s.item_name(),
                })),
                "position": view.position,
                "paused": view.paused,
                "bridged_as": bridged,
            })
        })
        .collect();
    sessions.sort_by(|a, b| {
        (a["user_name"].as_str(), a["device_name"].as_str())
            .cmp(&(b["user_name"].as_str(), b["device_name"].as_str()))
    });
    Json(serde_json::json!({
        "enabled": true,
        "error": snap.error,
        "sessions": sessions,
    }))
    .into_response()
}
