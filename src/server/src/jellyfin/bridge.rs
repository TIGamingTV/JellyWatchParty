//! Runs bridged Jellyfin devices as room members.
//!
//! Each bridged device is a normal client entry (`ClientKind::Bridge`) whose
//! outbound channel is read by a task here instead of a websocket. Whether
//! it drives or follows is simply whether it is the room's host right now,
//! so admin host changes, moves and kicks all go through the usual room
//! operations.
//!
//! One poller fetches `/Sessions` (only while bridges exist or an admin is
//! looking) and publishes the result on a `watch` channel; every bridge task
//! re-evaluates its device on each new snapshot.

use super::api::{JellyfinApi, JfSession};
use super::logic::{
    device_view, follower_step, host_step, local_checkin_ms, Command, FollowerMemory, HostEvent,
    HostMemory, RoomView, MAX_EXTRAPOLATION_SECS, TICKS_PER_SEC,
};
use super::time::parse_utc_ms;
use super::JellyfinConfig;
use crate::messaging::broadcast_room_list;
use crate::room::handle_leave;
use crate::room::ops::{self, AddOptions, OpError};
use crate::types::{
    Client, ClientKind, ClientMessageType, ClientReceiver, Clients, IncomingMessage, Rooms,
};
use crate::utils::{now_ms, random_token};
use log::{info, warn};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::task::{AbortHandle, JoinHandle};

/// Keep polling this long after the admin panel last asked for devices.
const ADMIN_VIEW_KEEPALIVE_MS: u64 = 30_000;
/// A bridged device missing from Jellyfin for this long leaves its room
/// (same grace a disconnected web client gets).
const DEVICE_GONE_AFTER_MS: u64 = 90_000;
const BRIDGE_CHANNEL_BUFFER: usize = 100;
/// Unpause receivers this long before a scheduled play, to cover the time a
/// device takes to act on the command.
const UNPAUSE_LEAD_MS: u64 = 300;
/// Clock offset samples older than this are dropped, so a clock that was
/// corrected is picked up again.
const CLOCK_SAMPLE_WINDOW_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Host,
    Receiver,
}

#[derive(Debug)]
pub enum AddError {
    Unavailable(String),
    SessionNotFound,
    RunsWebClient,
    NoRemoteControl,
    AlreadyBridged,
    Op(OpError),
}

impl AddError {
    pub fn message(&self) -> String {
        match self {
            AddError::Unavailable(e) => format!("Jellyfin is not reachable: {}", e),
            AddError::SessionNotFound => "That Jellyfin session is gone".into(),
            AddError::RunsWebClient => {
                "This client runs the Watch Party panel itself; add it as a client instead".into()
            }
            AddError::NoRemoteControl => {
                "This app doesn't accept remote control, so it can only be a host".into()
            }
            AddError::AlreadyBridged => {
                "This device is already in a room (from here or from the Watch Party panel)".into()
            }
            AddError::Op(e) => e.message().into(),
        }
    }
}

/// The latest `/Sessions` result.
#[derive(Debug, Default)]
pub struct Snapshot {
    pub sessions: Vec<JfSession>,
    pub fetched_at: u64,
    pub error: Option<String>,
    /// This server's clock minus Jellyfin's (ms), estimated from progress
    /// reports. Applied to every Jellyfin timestamp before use.
    pub clock_offset_ms: i64,
}

/// Estimates how far this server's clock is from Jellyfin's. Each time a
/// session's `LastPlaybackCheckIn` changes, the report happened before we
/// fetched it, so `fetched_at - checkin` is the offset plus a delay >= 0;
/// the smallest recent sample is the best estimate.
#[derive(Debug, Default)]
struct ClockEstimator {
    last_checkin: HashMap<String, u64>,
    samples: VecDeque<(u64, i64)>,
    offset: i64,
}

impl ClockEstimator {
    fn observe(&mut self, sessions: &[JfSession], fetched_at: u64) -> i64 {
        let mut seen = HashMap::new();
        for s in sessions {
            let Some(t) = s.last_playback_check_in.as_deref().and_then(parse_utc_ms) else {
                continue;
            };
            if let Some(prev) = self.last_checkin.get(&s.id) {
                if *prev != t {
                    self.samples
                        .push_back((fetched_at, fetched_at as i64 - t as i64));
                }
            }
            seen.insert(s.id.clone(), t);
        }
        self.last_checkin = seen;
        while self.samples.len() > 500
            || self
                .samples
                .front()
                .is_some_and(|(at, _)| fetched_at.saturating_sub(*at) > CLOCK_SAMPLE_WINDOW_MS)
        {
            self.samples.pop_front();
        }
        if let Some(min) = self.samples.iter().map(|(_, o)| *o).min() {
            self.offset = min;
        }
        self.offset
    }
}

/// What the admin panel shows about a bridged device.
#[derive(Debug, Clone)]
pub struct BridgeInfo {
    pub device_id: String,
    pub jf_user_id: String,
    pub session_id: String,
    pub device_name: String,
    pub client_name: String,
    pub remote_control: bool,
    pub status: &'static str,
    pub drift: Option<f64>,
    pub detail: Option<String>,
    /// Error from the last remote-control command, if it failed.
    pub cmd_error: Option<String>,
}

struct Entry {
    info: BridgeInfo,
    abort: Option<AbortHandle>,
}

struct Inner {
    api: JellyfinApi,
    poll_interval_ms: u64,
    clients: Clients,
    rooms: Rooms,
    entries: Mutex<HashMap<String, Entry>>,
    snapshot: watch::Sender<Arc<Snapshot>>,
    last_view_ms: AtomicU64,
    fetch_lock: tokio::sync::Mutex<()>,
    clock: Mutex<ClockEstimator>,
}

#[derive(Clone)]
pub struct Bridges(Arc<Inner>);

impl Bridges {
    /// Creates the bridge manager and starts its poller.
    pub fn start(cfg: &JellyfinConfig, clients: Clients, rooms: Rooms) -> Result<Self, String> {
        let bridges = Self::new(cfg, clients, rooms)?;
        let b = bridges.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(b.0.poll_interval_ms)).await;
                if b.wanted() {
                    b.refresh().await;
                }
            }
        });
        Ok(bridges)
    }

    fn new(cfg: &JellyfinConfig, clients: Clients, rooms: Rooms) -> Result<Self, String> {
        let (snapshot, _) = watch::channel(Arc::new(Snapshot::default()));
        Ok(Self(Arc::new(Inner {
            api: JellyfinApi::new(&cfg.url, &cfg.api_key)?,
            poll_interval_ms: cfg.poll_interval_ms,
            clients,
            rooms,
            entries: Mutex::new(HashMap::new()),
            snapshot,
            last_view_ms: AtomicU64::new(0),
            fetch_lock: tokio::sync::Mutex::new(()),
            clock: Mutex::new(ClockEstimator::default()),
        })))
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
        self.0.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn clock(&self) -> MutexGuard<'_, ClockEstimator> {
        self.0.clock.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn wanted(&self) -> bool {
        !self.entries().is_empty()
            || now_ms().saturating_sub(self.0.last_view_ms.load(Ordering::Relaxed))
                < ADMIN_VIEW_KEEPALIVE_MS
    }

    /// Fetches `/Sessions` now and publishes the result.
    pub async fn refresh(&self) -> Arc<Snapshot> {
        let _guard = self.0.fetch_lock.lock().await;
        let previous_error = self.0.snapshot.borrow().error.clone();
        let snap = match self.0.api.sessions().await {
            Ok(sessions) => {
                if let Some(e) = previous_error {
                    info!("Jellyfin reachable again (was: {})", e);
                }
                let fetched_at = now_ms();
                let clock_offset_ms = self.clock().observe(&sessions, fetched_at);
                Snapshot {
                    sessions,
                    fetched_at,
                    error: None,
                    clock_offset_ms,
                }
            }
            Err(e) => {
                if previous_error.as_deref() != Some(e.as_str()) {
                    warn!("Jellyfin session poll failed: {}", e);
                }
                Snapshot {
                    sessions: Vec::new(),
                    fetched_at: now_ms(),
                    error: Some(e),
                    clock_offset_ms: self.clock().offset,
                }
            }
        };
        let snap = Arc::new(snap);
        self.0.snapshot.send_replace(snap.clone());
        snap
    }

    /// The admin panel is looking: keep polling for a while, and return a
    /// snapshot no older than about one poll interval.
    pub async fn snapshot_for_admin(&self) -> Arc<Snapshot> {
        self.0.last_view_ms.store(now_ms(), Ordering::Relaxed);
        let current = self.0.snapshot.borrow().clone();
        if now_ms().saturating_sub(current.fetched_at) > self.0.poll_interval_ms * 2 {
            self.refresh().await
        } else {
            current
        }
    }

    /// The latest `/Sessions` result, without fetching.
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.0.snapshot.borrow().clone()
    }

    pub fn info(&self, client_id: &str) -> Option<BridgeInfo> {
        self.entries().get(client_id).map(|e| e.info.clone())
    }

    /// The Jellyfin REST client (shared with the chat integration).
    pub fn api(&self) -> &JellyfinApi {
        &self.0.api
    }

    /// Puts a Jellyfin session into a room. `Host` also makes it the host;
    /// `Receiver` needs a device that accepts remote control.
    pub async fn add(
        &self,
        room_id: &str,
        session_id: &str,
        role: Role,
    ) -> Result<String, AddError> {
        let snap = self.refresh().await;
        if let Some(e) = &snap.error {
            return Err(AddError::Unavailable(e.clone()));
        }
        let s = snap
            .sessions
            .iter()
            .find(|s| s.id == session_id && !s.is_own())
            .ok_or(AddError::SessionNotFound)?;
        if s.runs_web_client() {
            return Err(AddError::RunsWebClient);
        }
        if role == Role::Receiver && !s.supports_remote_control {
            return Err(AddError::NoRemoteControl);
        }
        let user_id = s.user_id();
        let client_id = uuid::Uuid::new_v4().to_string();

        // Reserve the device first, check and insert under one lock, so two
        // concurrent adds (a double click, two admin tabs) can't both pass.
        {
            let mut entries = self.entries();
            if entries.values().any(|e| e.info.device_id == s.device_id()) {
                return Err(AddError::AlreadyBridged);
            }
            entries.insert(
                client_id.clone(),
                Entry {
                    info: BridgeInfo {
                        device_id: s.device_id().to_string(),
                        jf_user_id: user_id.clone(),
                        session_id: s.id.clone(),
                        device_name: s.device_name().to_string(),
                        client_name: s.client_name().to_string(),
                        remote_control: s.supports_remote_control,
                        status: "loading",
                        drift: None,
                        detail: None,
                        cmd_error: None,
                    },
                    abort: None,
                },
            );
        }

        let (tx, rx) = mpsc::channel(BRIDGE_CHANNEL_BUFFER);
        let label = format!(
            "{} ({})",
            if s.user_name().is_empty() {
                "Jellyfin"
            } else {
                s.user_name()
            },
            if s.device_name().is_empty() {
                s.client_name()
            } else {
                s.device_name()
            }
        );
        let now = now_ms();
        {
            let mut rooms = self.0.rooms.write().await;
            let mut clients = self.0.clients.write().await;
            if !rooms.contains_key(room_id) {
                self.entries().remove(&client_id);
                return Err(AddError::Op(OpError::RoomNotFound));
            }
            // Bridged already, by this panel or by the plugin's in-panel
            // bridge (which tags its connection with the device id).
            if clients.values().any(|c| c.bridges_device(s.device_id())) {
                self.entries().remove(&client_id);
                return Err(AddError::AlreadyBridged);
            }
            clients.insert(
                client_id.clone(),
                Client {
                    sender: tx,
                    room_id: None,
                    user_id: user_id.clone(),
                    user_name: label.clone(),
                    authenticated: true,
                    message_count: 0,
                    last_reset: now,
                    last_seen: now,
                    resume_secret: random_token(),
                    connected_at: now,
                    conn_id: 0,
                    connected: true,
                    kind: ClientKind::Bridge,
                    bridge_device: Some(s.device_id().to_string()),
                },
            );
            let opts = AddOptions {
                by_admin: true,
                promote_if_hostless: role == Role::Host,
            };
            if let Err(e) = ops::add_member(room_id, &client_id, &mut rooms, &mut clients, opts) {
                clients.remove(&client_id);
                self.entries().remove(&client_id);
                return Err(AddError::Op(e));
            }
            if role == Role::Host {
                let _ = ops::set_host(room_id, &client_id, &mut rooms, &clients);
            }
        }

        let handle = tokio::spawn(run(self.clone(), client_id.clone(), rx));
        if let Some(e) = self.entries().get_mut(&client_id) {
            e.abort = Some(handle.abort_handle());
        }
        info!(
            "Bridged Jellyfin device '{}' into room {} as {:?} (client {})",
            label, room_id, role, client_id
        );
        broadcast_room_list(&self.0.clients, &self.0.rooms).await;
        Ok(client_id)
    }

    /// Stops a bridge and takes its device out of its room. Returns false
    /// if `client_id` isn't a bridge.
    pub async fn remove(&self, client_id: &str) -> bool {
        let Some(entry) = self.entries().remove(client_id) else {
            return false;
        };
        if let Some(a) = entry.abort {
            a.abort();
        }
        // Detach in its own task, so it completes even if the caller (an
        // admin HTTP request) goes away while waiting for the locks.
        let b = self.clone();
        let id = client_id.to_string();
        let _ = tokio::spawn(async move { b.detach(&id).await }).await;
        true
    }

    async fn detach(&self, client_id: &str) {
        {
            let mut rooms = self.0.rooms.write().await;
            let mut clients = self.0.clients.write().await;
            if clients.contains_key(client_id) {
                handle_leave(client_id, &mut clients, &mut rooms);
                clients.remove(client_id);
            }
        }
        broadcast_room_list(&self.0.clients, &self.0.rooms).await;
    }

    fn update(&self, client_id: &str, f: impl FnOnce(&mut BridgeInfo)) {
        if let Some(e) = self.entries().get_mut(client_id) {
            f(&mut e.info);
        }
    }

    async fn send_to_room(
        &self,
        client_id: &str,
        room_id: &str,
        msg_type: ClientMessageType,
        payload: serde_json::Value,
    ) {
        let msg = IncomingMessage {
            msg_type,
            room: Some(room_id.to_string()),
            client: Some(client_id.to_string()),
            payload: Some(payload),
            ts: now_ms(),
            server_ts: None,
        };
        crate::ws::dispatch_internal(client_id, msg, &self.0.clients, &self.0.rooms).await;
    }

    async fn run_command(&self, session_id: &str, cmd: &Command) -> Result<(), String> {
        let api = &self.0.api;
        match cmd {
            Command::PlayNow {
                item_id,
                start_secs,
            } => {
                api.play_now(session_id, item_id, (start_secs * TICKS_PER_SEC) as i64)
                    .await
            }
            Command::Seek { to_secs } => {
                api.playstate(session_id, "Seek", Some((to_secs * TICKS_PER_SEC) as i64))
                    .await
            }
            Command::Pause => api.playstate(session_id, "Pause", None).await,
            Command::Unpause => api.playstate(session_id, "Unpause", None).await,
        }
    }

    /// One evaluation of a bridged device. Returns why the bridge should
    /// stop, if it should.
    async fn tick(
        &self,
        client_id: &str,
        snap: &Snapshot,
        st: &mut TaskState,
    ) -> Option<&'static str> {
        let now = now_ms();
        let (room_id, view, is_host, ready, started) = {
            let rooms = self.0.rooms.read().await;
            let clients = self.0.clients.read().await;
            let Some(client) = clients.get(client_id) else {
                return Some("removed");
            };
            let Some(room_id) = client.room_id.clone() else {
                return Some("no longer in a room");
            };
            let Some(room) = rooms.get(&room_id) else {
                return Some("room closed");
            };
            let mut playing = room.state.play_state == "playing" && room.pending_play.is_none();
            let mut expected = if playing {
                room.state.position + now.saturating_sub(room.last_state_ts) as f64 / 1000.0
            } else {
                room.state.position
            };
            let mut hold = false;
            // A scheduled play (countdown, or the usual short delay) starts at
            // its target time, not when it was announced: until the room
            // reports a newer state, measure from the target.
            if let Some((target, pos)) = st.anchor {
                if room.last_state_ts > target || room.state.play_state != "playing" {
                    st.anchor = None;
                } else {
                    playing = true;
                    if now + UNPAUSE_LEAD_MS < target {
                        hold = true;
                        expected = pos;
                    } else {
                        expected = pos + now.saturating_sub(target) as f64 / 1000.0;
                    }
                }
            }
            let view = RoomView::new(room.media_id.as_deref(), playing, expected, hold);
            (
                room_id,
                view,
                room.host_id == client_id,
                room.ready_clients.contains(client_id),
                room.started,
            )
        };

        if let Some(err) = &snap.error {
            let err = err.clone();
            self.update(client_id, |i| {
                i.status = "error";
                i.drift = None;
                i.detail = Some(err);
            });
            self.report_status(client_id, &room_id, "idle", st).await;
            return None;
        }

        let Some(info) = self.info(client_id) else {
            return Some("removed");
        };
        let session = snap
            .sessions
            .iter()
            .find(|s| s.device_id() == info.device_id && s.user_id() == info.jf_user_id);
        match session {
            None => {
                let since = *st.missing_since.get_or_insert(now);
                if now.saturating_sub(since) > DEVICE_GONE_AFTER_MS {
                    return Some("device gone from Jellyfin");
                }
            }
            Some(s) => {
                st.missing_since = None;
                let (sid, rc) = (s.id.clone(), s.supports_remote_control);
                self.update(client_id, |i| {
                    i.session_id = sid;
                    i.remote_control = rc;
                });
                st.note_checkin(local_checkin_ms(s, snap.clock_offset_ms));
            }
        }
        let max_extrapolation = st.max_extrapolation_secs();
        let device = session.map(|s| device_view(s, now, snap.clock_offset_ms, max_extrapolation));

        if st.was_host != Some(is_host) {
            st.was_host = Some(is_host);
            st.host = HostMemory::default();
            st.follower = FollowerMemory::default();
        }

        let (status, drift, detail) = if is_host {
            if !started {
                // A device can't hold its first play for the start
                // countdown; it is already playing. Start the room as is.
                if let Some(room) = self.0.rooms.write().await.get_mut(&room_id) {
                    room.started = true;
                }
            }
            if !ready && device.as_ref().is_some_and(|d| d.item_id.is_some()) {
                self.send_to_room(
                    client_id,
                    &room_id,
                    ClientMessageType::Ready,
                    serde_json::json!({}),
                )
                .await;
            }
            let (events, status) = host_step(&view, device.as_ref(), &mut st.host);
            for ev in events {
                let (ty, payload) = match ev {
                    HostEvent::SetMedia { media_id, position } => (
                        ClientMessageType::SetMedia,
                        serde_json::json!({ "media_id": media_id, "position": position }),
                    ),
                    HostEvent::Play { position } => (
                        ClientMessageType::PlayerEvent,
                        serde_json::json!({ "action": "play", "position": position }),
                    ),
                    HostEvent::Pause { position } => (
                        ClientMessageType::PlayerEvent,
                        serde_json::json!({ "action": "pause", "position": position }),
                    ),
                    HostEvent::State { position, playing } => (
                        ClientMessageType::StateUpdate,
                        serde_json::json!({
                            "position": position,
                            "play_state": if playing { "playing" } else { "paused" }
                        }),
                    ),
                };
                self.send_to_room(client_id, &room_id, ty, payload).await;
            }
            (status, None, None)
        } else if session.is_some_and(|s| !s.supports_remote_control) {
            (
                "error",
                None,
                Some(
                    "This app doesn't accept remote control; make it the host instead".to_string(),
                ),
            )
        } else if st.inflight.as_ref().is_some_and(|h| !h.is_finished()) {
            // Commands from the last tick are still on their way; decide
            // again once they're through.
            (info.status, info.drift, info.detail.clone())
        } else {
            let step = follower_step(&view, device.as_ref(), &mut st.follower, now);
            if let (Some(s), false) = (session, step.commands.is_empty()) {
                st.inflight = Some(self.spawn_commands(client_id, &s.id, step.commands.clone()));
            }
            if step.on_item && !ready {
                self.send_to_room(
                    client_id,
                    &room_id,
                    ClientMessageType::Ready,
                    serde_json::json!({}),
                )
                .await;
            }
            let detail = step
                .note
                .map(String::from)
                .or_else(|| info.cmd_error.clone());
            (step.status, step.drift, detail)
        };

        let room_status = match status {
            "offline" | "error" => "idle",
            s => s,
        };
        self.report_status(client_id, &room_id, room_status, st)
            .await;
        self.update(client_id, |i| {
            i.status = status;
            i.drift = drift;
            i.detail = detail;
        });
        None
    }

    /// Sends remote-control commands in their own task, so the bridge keeps
    /// reading room messages while Jellyfin answers (up to its timeout).
    fn spawn_commands(
        &self,
        client_id: &str,
        session_id: &str,
        cmds: Vec<Command>,
    ) -> JoinHandle<()> {
        let b = self.clone();
        let client_id = client_id.to_string();
        let session_id = session_id.to_string();
        tokio::spawn(async move {
            let mut error = None;
            for cmd in &cmds {
                if let Err(e) = b.run_command(&session_id, cmd).await {
                    let repeated = b
                        .info(&client_id)
                        .is_some_and(|i| i.cmd_error.as_deref() == Some(e.as_str()));
                    if !repeated {
                        warn!("Bridge {}: {:?} failed: {}", client_id, cmd, e);
                    }
                    error = Some(e);
                    break;
                }
            }
            b.update(&client_id, |i| i.cmd_error = error);
        })
    }

    /// Shows the device's state in the room's participant list.
    async fn report_status(
        &self,
        client_id: &str,
        room_id: &str,
        status: &'static str,
        st: &mut TaskState,
    ) {
        if st.reported.as_ref() == Some(&(room_id.to_string(), status)) {
            return;
        }
        st.reported = Some((room_id.to_string(), status));
        self.send_to_room(
            client_id,
            room_id,
            ClientMessageType::ClientStatus,
            serde_json::json!({ "status": status }),
        )
        .await;
    }
}

#[derive(Default)]
struct TaskState {
    host: HostMemory,
    follower: FollowerMemory,
    was_host: Option<bool>,
    /// The room's latest scheduled play: `(target ms, position)`.
    anchor: Option<(u64, f64)>,
    missing_since: Option<u64>,
    reported: Option<(String, &'static str)>,
    /// Remote-control commands still being sent.
    inflight: Option<JoinHandle<()>>,
    /// Last progress report seen (local ms) and the usual gap between them.
    last_checkin: u64,
    report_interval_ms: Option<u64>,
}

impl TaskState {
    fn note_checkin(&mut self, checkin: u64) {
        if checkin > self.last_checkin {
            if self.last_checkin > 0 {
                let gap = checkin - self.last_checkin;
                self.report_interval_ms = Some(match self.report_interval_ms {
                    Some(prev) => (prev * 3 + gap) / 4,
                    None => gap,
                });
            }
            self.last_checkin = checkin;
        }
    }

    /// Extrapolate up to three report intervals (30-120 s): a device that
    /// reports rarely isn't mistaken for a stalled one.
    fn max_extrapolation_secs(&self) -> f64 {
        self.report_interval_ms
            .map(|ms| (ms as f64 * 3.0 / 1000.0).clamp(MAX_EXTRAPOLATION_SECS, 120.0))
            .unwrap_or(MAX_EXTRAPOLATION_SECS)
    }

    /// How long until the receiver should be unpaused for a scheduled
    /// play, if one is coming up.
    fn wake_in(&self, now: u64) -> Option<Duration> {
        let (target, _) = self.anchor?;
        let at = target.saturating_sub(UNPAUSE_LEAD_MS);
        (at > now).then(|| Duration::from_millis(at - now))
    }
}

/// Notes scheduled plays (and what cancels them) from the room's messages.
fn note_room_message(text: &str, st: &mut TaskState) {
    let Ok(msg) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    match msg["type"].as_str() {
        Some("player_event") => match msg["payload"]["action"].as_str() {
            Some("play") => {
                if let (Some(target), Some(pos)) = (
                    msg["payload"]["target_server_ts"].as_u64(),
                    msg["payload"]["position"].as_f64(),
                ) {
                    st.anchor = Some((target, pos));
                }
            }
            _ => st.anchor = None,
        },
        Some("room_state") | Some("media_changed") => st.anchor = None,
        _ => {}
    }
}

async fn run(bridges: Bridges, client_id: String, mut rx: ClientReceiver) {
    let mut snaps = bridges.0.snapshot.subscribe();
    let mut st = TaskState::default();
    let reason = loop {
        let wake = st.wake_in(now_ms());
        tokio::select! {
            msg = rx.recv() => match msg {
                None => break "removed",
                Some(m) => {
                    note_room_message(&m.into_text(), &mut st);
                    continue;
                }
            },
            changed = snaps.changed() => {
                if changed.is_err() {
                    break "shutting down";
                }
            }
            // Unpause right when a scheduled play starts, not on the next poll.
            _ = tokio::time::sleep(wake.unwrap_or_default()), if wake.is_some() => {}
        }
        let snap = snaps.borrow_and_update().clone();
        if let Some(reason) = bridges.tick(&client_id, &snap, &mut st).await {
            break reason;
        }
    };
    info!("Bridge {} stopped: {}", client_id, reason);
    let was_registered = bridges.entries().remove(&client_id).is_some();
    if was_registered {
        bridges.detach(&client_id).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jellyfin::api::{JfItem, JfPlayState};
    use crate::test_helpers;

    const ITEM: &str = "0123456789abcdef0123456789abcdef";

    fn bridges() -> Bridges {
        Bridges::new(
            &JellyfinConfig {
                url: "http://127.0.0.1:9".into(),
                api_key: "k".into(),
                poll_interval_ms: 1000,
            },
            test_helpers::create_clients(),
            test_helpers::create_rooms(),
        )
        .unwrap()
    }

    fn session(id: &str, item: Option<&str>, paused: bool, remote: bool) -> JfSession {
        JfSession {
            id: id.into(),
            user_id: Some("u1".into()),
            user_name: Some("Alice".into()),
            client: Some("Android TV".into()),
            device_name: Some("TV".into()),
            device_id: Some(format!("dev-{}", id)),
            supports_remote_control: remote,
            now_playing_item: item.map(|i| JfItem {
                id: Some(i.into()),
                ..Default::default()
            }),
            play_state: Some(JfPlayState {
                position_ticks: Some(10 * 10_000_000),
                is_paused: paused,
            }),
            last_playback_check_in: None,
            last_activity_date: None,
        }
    }

    /// Registers a bridge member by hand (what `add` does after its Jellyfin
    /// lookup), without spawning its task, so `tick` can be driven directly.
    async fn add_member(
        b: &Bridges,
        room: &str,
        s: &JfSession,
        host: bool,
    ) -> (String, ClientReceiver) {
        let (tx, rx) = mpsc::channel(100);
        let id = format!("bridge-{}", s.id);
        {
            let mut rooms = b.0.rooms.write().await;
            let mut clients = b.0.clients.write().await;
            let (mut c, _) = test_helpers::create_client_with_rx("u1", "Alice (TV)", true);
            c.sender = tx;
            c.kind = ClientKind::Bridge;
            clients.insert(id.clone(), c);
            let opts = AddOptions {
                by_admin: true,
                promote_if_hostless: host,
            };
            ops::add_member(room, &id, &mut rooms, &mut clients, opts).unwrap();
            if host {
                ops::set_host(room, &id, &mut rooms, &clients).unwrap();
            }
        }
        b.entries().insert(
            id.clone(),
            Entry {
                info: BridgeInfo {
                    device_id: s.device_id().into(),
                    jf_user_id: s.user_id(),
                    session_id: s.id.clone(),
                    device_name: "TV".into(),
                    client_name: "Android TV".into(),
                    remote_control: s.supports_remote_control,
                    status: "loading",
                    drift: None,
                    detail: None,
                    cmd_error: None,
                },
                abort: None,
            },
        );
        (id, rx)
    }

    fn snap(sessions: Vec<JfSession>) -> Snapshot {
        Snapshot {
            sessions,
            fetched_at: now_ms(),
            error: None,
            clock_offset_ms: 0,
        }
    }

    #[tokio::test]
    async fn host_bridge_drives_the_room() {
        let b = bridges();
        let room = ops::create_group("G", None, &mut *b.0.rooms.write().await).unwrap();
        let s = session("s1", Some(ITEM), false, false);
        let (id, _rx) = add_member(&b, &room, &s, true).await;
        let mut st = TaskState::default();

        assert_eq!(b.tick(&id, &snap(vec![s.clone()]), &mut st).await, None);

        let rooms = b.0.rooms.read().await;
        let r = &rooms[&room];
        assert_eq!(r.host_id, id);
        assert_eq!(r.media_id.as_deref(), Some(ITEM));
        assert_eq!(r.state.play_state, "playing");
        assert_eq!(
            r.client_status.get(&id).map(String::as_str),
            Some("playing")
        );
        drop(rooms);
        assert_eq!(b.info(&id).unwrap().status, "playing");
    }

    #[tokio::test]
    async fn receiver_without_remote_control_reports_an_error() {
        let b = bridges();
        let room = ops::create_group("G", None, &mut *b.0.rooms.write().await).unwrap();
        let s = session("s2", None, true, false);
        let (id, _rx) = add_member(&b, &room, &s, false).await;
        let mut st = TaskState::default();
        assert_eq!(b.tick(&id, &snap(vec![s]), &mut st).await, None);
        let info = b.info(&id).unwrap();
        assert_eq!(info.status, "error");
        assert!(info.detail.unwrap().contains("remote control"));
    }

    #[tokio::test]
    async fn receiver_with_nothing_to_play_is_ready_and_idle() {
        let b = bridges();
        let room = ops::create_group("G", None, &mut *b.0.rooms.write().await).unwrap();
        let s = session("s3", None, true, true);
        let (id, _rx) = add_member(&b, &room, &s, false).await;
        let mut st = TaskState::default();
        assert_eq!(b.tick(&id, &snap(vec![s]), &mut st).await, None);
        assert!(b.0.rooms.read().await[&room].ready_clients.contains(&id));
        assert_eq!(b.info(&id).unwrap().status, "idle");
    }

    #[tokio::test]
    async fn jellyfin_errors_are_shown_not_acted_on() {
        let b = bridges();
        let room = ops::create_group("G", None, &mut *b.0.rooms.write().await).unwrap();
        let s = session("s4", None, true, true);
        let (id, _rx) = add_member(&b, &room, &s, false).await;
        let mut st = TaskState::default();
        let bad = Snapshot {
            sessions: vec![],
            fetched_at: now_ms(),
            error: Some("boom".into()),
            clock_offset_ms: 0,
        };
        assert_eq!(b.tick(&id, &bad, &mut st).await, None);
        let info = b.info(&id).unwrap();
        assert_eq!(info.status, "error");
        assert_eq!(info.detail.as_deref(), Some("boom"));
        assert!(
            st.missing_since.is_none(),
            "errors don't count as the device being gone"
        );
    }

    #[tokio::test]
    async fn a_device_missing_for_too_long_stops_the_bridge() {
        let b = bridges();
        let room = ops::create_group("G", None, &mut *b.0.rooms.write().await).unwrap();
        let s = session("s5", None, true, true);
        let (id, _rx) = add_member(&b, &room, &s, false).await;
        let mut st = TaskState::default();
        assert_eq!(b.tick(&id, &snap(vec![]), &mut st).await, None);
        st.missing_since = Some(now_ms() - DEVICE_GONE_AFTER_MS - 1);
        assert_eq!(
            b.tick(&id, &snap(vec![]), &mut st).await,
            Some("device gone from Jellyfin")
        );
    }

    #[tokio::test]
    async fn a_closed_room_stops_the_bridge_and_remove_cleans_up() {
        let b = bridges();
        let room = ops::create_group("G", None, &mut *b.0.rooms.write().await).unwrap();
        let s = session("s6", None, true, true);
        let (id, _rx) = add_member(&b, &room, &s, false).await;
        {
            let mut rooms = b.0.rooms.write().await;
            let mut clients = b.0.clients.write().await;
            ops::close_room(&room, "x", &mut rooms, &mut clients).unwrap();
        }
        let mut st = TaskState::default();
        assert_eq!(
            b.tick(&id, &snap(vec![s]), &mut st).await,
            Some("no longer in a room")
        );

        assert!(b.remove(&id).await);
        assert!(!b.0.clients.read().await.contains_key(&id));
        assert!(b.info(&id).is_none());
        assert!(!b.remove(&id).await);
    }

    /// A fake Jellyfin: serves `/Sessions` from shared state and records the
    /// remote-control calls it receives.
    mod mock {
        use super::super::super::api::OWN_DEVICE_ID;
        use axum::extract::{Path, Query, State};
        use axum::http::{HeaderMap, StatusCode};
        use axum::routing::{get, post};
        use axum::{Json, Router};
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        #[derive(Clone, Default)]
        pub struct Mock {
            pub sessions: Arc<Mutex<serde_json::Value>>,
            pub calls: Arc<Mutex<Vec<String>>>,
        }

        fn authorized(h: &HeaderMap) -> bool {
            h.get("authorization")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| {
                    v.starts_with("MediaBrowser ")
                        && v.contains("Token=\"test-key\"")
                        && v.contains(OWN_DEVICE_ID)
                })
        }

        async fn sessions(
            State(m): State<Mock>,
            h: HeaderMap,
        ) -> Result<Json<serde_json::Value>, StatusCode> {
            if !authorized(&h) {
                return Err(StatusCode::UNAUTHORIZED);
            }
            Ok(Json(m.sessions.lock().unwrap().clone()))
        }

        async fn playstate(
            State(m): State<Mock>,
            h: HeaderMap,
            Path((id, cmd)): Path<(String, String)>,
            Query(q): Query<HashMap<String, String>>,
        ) -> StatusCode {
            if !authorized(&h) {
                return StatusCode::UNAUTHORIZED;
            }
            let ticks = q.get("seekPositionTicks").cloned().unwrap_or_default();
            m.calls
                .lock()
                .unwrap()
                .push(format!("{} {} {}", id, cmd, ticks));
            StatusCode::NO_CONTENT
        }

        async fn play(
            State(m): State<Mock>,
            h: HeaderMap,
            Path(id): Path<String>,
            Query(q): Query<HashMap<String, String>>,
        ) -> StatusCode {
            if !authorized(&h) {
                return StatusCode::UNAUTHORIZED;
            }
            m.calls.lock().unwrap().push(format!(
                "{} {} {} {}",
                id, q["playCommand"], q["itemIds"], q["startPositionTicks"]
            ));
            StatusCode::NO_CONTENT
        }

        pub async fn spawn(m: Mock) -> String {
            let app = Router::new()
                .route("/Sessions", get(sessions))
                .route("/Sessions/{id}/Playing", post(play))
                .route("/Sessions/{id}/Playing/{cmd}", post(playstate))
                .with_state(m);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });
            format!("http://{}", addr)
        }
    }

    fn tv_json(item: Option<&str>, position_secs: i64, paused: bool) -> serde_json::Value {
        serde_json::json!([{
            "Id": "tv-session",
            "UserId": "AAAA-BBBB",
            "UserName": "Bob",
            "Client": "Android TV",
            "DeviceName": "Living room",
            "DeviceId": "tv-device",
            "SupportsRemoteControl": true,
            "NowPlayingItem": item.map(|i| serde_json::json!({ "Id": i, "Name": "Movie" })),
            "PlayState": { "PositionTicks": position_secs * 10_000_000, "IsPaused": paused }
        }, {
            "Id": "web", "Client": "Jellyfin Web 12.0", "DeviceId": "browser", "UserId": "x"
        }, {
            "Id": "self", "Client": "JellyWatchParty Session Server", "DeviceId": crate::jellyfin::api::OWN_DEVICE_ID
        }])
    }

    async fn wait_for(cond: impl Fn() -> bool) {
        for _ in 0..40 {
            if cond() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    #[tokio::test]
    async fn receiver_end_to_end_against_a_fake_jellyfin() {
        let m = mock::Mock::default();
        *m.sessions.lock().unwrap() = tv_json(None, 0, true);
        let url = mock::spawn(m.clone()).await;
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let b = Bridges::start(
            &JellyfinConfig {
                url,
                api_key: "test-key".into(),
                poll_interval_ms: 250,
            },
            clients.clone(),
            rooms.clone(),
        )
        .unwrap();

        // A web host watching ITEM at 600 s, paused.
        {
            let mut lr = rooms.write().await;
            let mut lc = clients.write().await;
            let _rx = test_helpers::setup_room_with_host(&mut lc, &mut lr, "web-host");
            let r = lr.get_mut("room-1").unwrap();
            r.media_id = Some(ITEM.into());
            r.state.position = 600.0;
        }

        // Web clients and the server's own session can't be bridged.
        assert!(matches!(
            b.add("room-1", "web", Role::Receiver).await,
            Err(AddError::RunsWebClient)
        ));
        assert!(matches!(
            b.add("room-1", "self", Role::Receiver).await,
            Err(AddError::SessionNotFound)
        ));

        // A plugin bridge already stands in for the TV: refused.
        {
            let (mut plugin, _rx) =
                test_helpers::create_client_with_rx("aaaabbbb", "Bob (TV)", true);
            plugin.bridge_device = Some("tv-device".into());
            plugin.room_id = Some("room-x".into());
            clients.write().await.insert("plugin-bridge".into(), plugin);
        }
        assert!(matches!(
            b.add("room-1", "tv-session", Role::Receiver).await,
            Err(AddError::AlreadyBridged)
        ));
        // Removed from its room (tag left behind): no longer counts.
        clients
            .write()
            .await
            .get_mut("plugin-bridge")
            .unwrap()
            .room_id = None;

        let id = b.add("room-1", "tv-session", Role::Receiver).await.unwrap();
        assert!(matches!(
            b.add("room-1", "tv-session", Role::Receiver).await,
            Err(AddError::AlreadyBridged)
        ));
        assert!(clients.read().await[&id].bridges_device("tv-device"));
        clients.write().await.remove("plugin-bridge");

        // Idle TV: told to play the room's item from the room's position.
        let calls = m.calls.clone();
        wait_for(|| !calls.lock().unwrap().is_empty()).await;
        assert_eq!(
            calls.lock().unwrap()[0],
            format!("tv-session PlayNow {} {}", ITEM, 600i64 * 10_000_000)
        );

        // Now on the item but far off: seeked (room paused, so no lead).
        *m.sessions.lock().unwrap() = tv_json(Some(&ITEM.to_uppercase()), 10, true);
        wait_for(|| calls.lock().unwrap().iter().any(|c| c.contains("Seek"))).await;
        assert!(calls
            .lock()
            .unwrap()
            .contains(&format!("tv-session Seek {}", 600i64 * 10_000_000)));
        assert!(rooms.read().await["room-1"].ready_clients.contains(&id));

        // Removing it takes it out of the room and stops the task.
        assert!(b.remove(&id).await);
        assert!(!rooms.read().await["room-1"].clients.contains(&id));
        assert!(!clients.read().await.contains_key(&id));
    }

    #[tokio::test]
    async fn bad_api_key_is_reported() {
        let m = mock::Mock::default();
        let url = mock::spawn(m).await;
        let b = Bridges::new(
            &JellyfinConfig {
                url,
                api_key: "wrong".into(),
                poll_interval_ms: 1000,
            },
            test_helpers::create_clients(),
            test_helpers::create_rooms(),
        )
        .unwrap();
        let snap = b.refresh().await;
        assert!(snap.error.as_deref().unwrap().contains("JELLYFIN_API_KEY"));
        assert!(matches!(
            b.add("room", "x", Role::Host).await,
            Err(AddError::Unavailable(_))
        ));
    }

    #[test]
    fn scheduled_plays_are_anchored_and_cancelled() {
        let mut st = TaskState::default();
        let play = serde_json::json!({
            "type": "player_event",
            "payload": { "action": "play", "position": 12.5, "target_server_ts": 5_000 }
        });
        note_room_message(&play.to_string(), &mut st);
        assert_eq!(st.anchor, Some((5_000, 12.5)));
        assert_eq!(st.wake_in(4_000), Some(Duration::from_millis(700)));
        assert_eq!(st.wake_in(4_800), None, "inside the unpause lead");
        note_room_message(
            r#"{"type":"player_event","payload":{"action":"pause","position":1}}"#,
            &mut st,
        );
        assert_eq!(st.anchor, None);
        // Read late (after its target): still anchors, for the position.
        note_room_message(&play.to_string(), &mut st);
        assert_eq!(st.anchor, Some((5_000, 12.5)));
        note_room_message(r#"{"type":"media_changed"}"#, &mut st);
        assert_eq!(st.anchor, None);
    }

    #[test]
    fn clock_offset_is_the_smallest_fresh_sample() {
        let mut c = ClockEstimator::default();
        let mut s = session("s", Some(ITEM), false, true);
        s.last_playback_check_in = Some("1970-01-01T00:00:10Z".into());
        // First sighting: no sample (the report could be old).
        assert_eq!(c.observe(&[s.clone()], 50_000), 0);
        // Jellyfin's clock is 5 s behind ours: reports land 5 s "late" plus
        // up to a poll interval.
        s.last_playback_check_in = Some("1970-01-01T00:00:13Z".into());
        assert_eq!(c.observe(&[s.clone()], 18_700), 5_700);
        s.last_playback_check_in = Some("1970-01-01T00:00:16Z".into());
        assert_eq!(c.observe(&[s.clone()], 21_100), 5_100);
        // Unchanged report: no new sample.
        assert_eq!(c.observe(&[s.clone()], 23_000), 5_100);
        // Old samples expire.
        s.last_playback_check_in = Some("1970-01-01T00:00:19Z".into());
        let later = 21_100 + CLOCK_SAMPLE_WINDOW_MS + 1;
        let o = c.observe(&[s], later);
        assert_eq!(o, later as i64 - 19_000);
    }

    #[test]
    fn extrapolation_cap_follows_the_report_interval() {
        let mut st = TaskState::default();
        assert_eq!(st.max_extrapolation_secs(), MAX_EXTRAPOLATION_SECS);
        st.note_checkin(1_000);
        st.note_checkin(41_000);
        assert_eq!(st.max_extrapolation_secs(), 120.0);
        let mut quick = TaskState::default();
        quick.note_checkin(1_000);
        quick.note_checkin(4_000);
        assert_eq!(quick.max_extrapolation_secs(), MAX_EXTRAPOLATION_SECS);
    }

    #[tokio::test]
    async fn a_device_host_starts_the_room_without_a_countdown() {
        let b = bridges();
        let room = ops::create_group("G", None, &mut *b.0.rooms.write().await).unwrap();
        b.0.rooms.write().await.get_mut(&room).unwrap().started = false;
        let s = session("s7", Some(ITEM), false, false);
        let (id, _rx) = add_member(&b, &room, &s, true).await;
        let mut st = TaskState::default();
        assert_eq!(b.tick(&id, &snap(vec![s]), &mut st).await, None);
        assert!(b.0.rooms.read().await[&room].started);
    }

    #[tokio::test]
    async fn receivers_wait_for_a_scheduled_play_then_follow_from_its_target() {
        let b = bridges();
        let room = ops::create_group("G", None, &mut *b.0.rooms.write().await).unwrap();
        let s = session("s8", Some(ITEM), true, true);
        let (id, _rx) = add_member(&b, &room, &s, false).await;
        let now = now_ms();
        {
            let mut lr = b.0.rooms.write().await;
            let r = lr.get_mut(&room).unwrap();
            r.media_id = Some(ITEM.into());
            r.state.position = 10.0;
            r.state.play_state = "playing".into();
            // Announced 4 s ago (before either target below).
            r.last_state_ts = now - 4_000;
        }
        // Countdown: play at 10 s, starting 3 s from now. The paused TV is
        // at 10 s: in sync, and not unpaused early.
        let mut st = TaskState {
            anchor: Some((now + 3_000, 10.0)),
            ..Default::default()
        };
        assert_eq!(b.tick(&id, &snap(vec![s.clone()]), &mut st).await, None);
        assert!(st.inflight.is_none(), "no command before the start");
        assert_eq!(b.info(&id).unwrap().status, "synced");

        // After the target, the room counts from the target (11 s), not
        // from when the play was announced (which would say 14 s).
        let mut st = TaskState {
            anchor: Some((now - 1_000, 10.0)),
            ..Default::default()
        };
        assert_eq!(b.tick(&id, &snap(vec![s]), &mut st).await, None);
        let drift = b.info(&id).unwrap().drift.unwrap();
        assert!((drift + 1.0).abs() < 0.2, "drift {}", drift);
    }
}
