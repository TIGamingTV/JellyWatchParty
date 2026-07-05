mod auth;
mod chat;
mod create;
mod join;
mod misc;
mod playback;

pub(in crate::ws) use auth::handle_auth;
pub(in crate::ws) use chat::handle_chat_message;
pub(in crate::ws) use create::handle_create_room;
pub(in crate::ws) use join::handle_join_room;
pub(in crate::ws) use misc::{
    handle_client_log, handle_leave_room, handle_ping, handle_ready, handle_toggle_democratic_mode,
    handle_unknown,
};
pub(in crate::ws) use playback::handle_playback;
