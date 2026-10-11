//! Custom ids of buttons, select menus and modals: `jwp:v1:<action>[:<room>]`.
//!
//! Anyone can send any custom id to the bot, so these are only hints about
//! what was clicked: the server checks every action again.

const PREFIX: &str = "jwp:v1:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Id {
    // Panel buttons.
    Join(String),
    AddDevice(String),
    RemoveDevice(String),
    PickHost(String),
    Leave(String),
    Close(String),
    // Private follow-ups.
    ConfirmClose(String),
    DevicePick(String),
    HostPick(String),
    RemovePick(String),
    // Modals.
    LinkModal,
    CreateModal,
    JoinModal(String),
    PasswordModal(String),
}

pub use crate::api::valid_room_id;

impl Id {
    pub fn encode(&self) -> String {
        let (action, room) = match self {
            Id::Join(r) => ("join", Some(r)),
            Id::AddDevice(r) => ("dev", Some(r)),
            Id::RemoveDevice(r) => ("undev", Some(r)),
            Id::PickHost(r) => ("host", Some(r)),
            Id::Leave(r) => ("leave", Some(r)),
            Id::Close(r) => ("close", Some(r)),
            Id::ConfirmClose(r) => ("closeok", Some(r)),
            Id::DevicePick(r) => ("devpick", Some(r)),
            Id::HostPick(r) => ("hostpick", Some(r)),
            Id::RemovePick(r) => ("undevpick", Some(r)),
            Id::LinkModal => ("m-link", None),
            Id::CreateModal => ("m-create", None),
            Id::JoinModal(r) => ("m-join", Some(r)),
            Id::PasswordModal(r) => ("m-pw", Some(r)),
        };
        match room {
            Some(r) => format!("{}{}:{}", PREFIX, action, r),
            None => format!("{}{}", PREFIX, action),
        }
    }

    pub fn parse(s: &str) -> Option<Id> {
        let rest = s.strip_prefix(PREFIX)?;
        let (action, room) = match rest.split_once(':') {
            Some((a, r)) => (a, Some(r)),
            None => (rest, None),
        };
        if let Some(r) = room {
            if !valid_room_id(r) {
                return None;
            }
        }
        let r = || room.map(str::to_string);
        Some(match (action, room.is_some()) {
            ("join", true) => Id::Join(r()?),
            ("dev", true) => Id::AddDevice(r()?),
            ("undev", true) => Id::RemoveDevice(r()?),
            ("host", true) => Id::PickHost(r()?),
            ("leave", true) => Id::Leave(r()?),
            ("close", true) => Id::Close(r()?),
            ("closeok", true) => Id::ConfirmClose(r()?),
            ("devpick", true) => Id::DevicePick(r()?),
            ("hostpick", true) => Id::HostPick(r()?),
            ("undevpick", true) => Id::RemovePick(r()?),
            ("m-link", false) => Id::LinkModal,
            ("m-create", false) => Id::CreateModal,
            ("m-join", true) => Id::JoinModal(r()?),
            ("m-pw", true) => Id::PasswordModal(r()?),
            _ => return None,
        })
    }
}

/// A device choice in the "add my device" menu: `h:<session>` / `r:<session>`.
pub fn encode_device_choice(session_id: &str, host: bool) -> String {
    format!("{}:{}", if host { "h" } else { "r" }, session_id)
}

pub fn parse_device_choice(v: &str) -> Option<(String, bool)> {
    let (role, session) = v.split_once(':')?;
    let host = match role {
        "h" => true,
        "r" => false,
        _ => return None,
    };
    let ok = !session.is_empty()
        && session.len() <= 64
        && session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    ok.then(|| (session.to_string(), host))
}

/// A "kick" choice: `m:<member id>` (a device or viewer) or `u:<account>`.
pub fn parse_kick_choice(v: &str) -> Option<(bool, String)> {
    let (kind, id) = v.split_once(':')?;
    if id.is_empty() || id.len() > 64 {
        return None;
    }
    match kind {
        "m" => Some((true, id.to_string())),
        "u" if id.bytes().all(|b| b.is_ascii_digit()) => Some((false, id.to_string())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: &str = "7b0d3c1e-0c39-4a43-9a0e-2f1f3a3e9d10";

    #[test]
    fn ids_round_trip_and_fit_discords_limit() {
        let all = [
            Id::Join(ROOM.into()),
            Id::AddDevice(ROOM.into()),
            Id::RemoveDevice(ROOM.into()),
            Id::PickHost(ROOM.into()),
            Id::Leave(ROOM.into()),
            Id::Close(ROOM.into()),
            Id::ConfirmClose(ROOM.into()),
            Id::DevicePick(ROOM.into()),
            Id::HostPick(ROOM.into()),
            Id::RemovePick(ROOM.into()),
            Id::LinkModal,
            Id::CreateModal,
            Id::JoinModal(ROOM.into()),
            Id::PasswordModal(ROOM.into()),
        ];
        for id in all {
            let s = id.encode();
            assert!(s.len() <= 100, "{}", s);
            assert_eq!(Id::parse(&s), Some(id));
        }
    }

    #[test]
    fn foreign_or_forged_ids_are_ignored() {
        for s in [
            "",
            "jwp:v1:",
            "jwp:v2:join:7b0d3c1e-0c39-4a43-9a0e-2f1f3a3e9d10",
            "other:join",
            "jwp:v1:join",
            "jwp:v1:join:../../admin",
            "jwp:v1:join:7b0d3c1e-0c39-4a43-9a0e-2f1f3a3e9d1Z",
            "jwp:v1:m-link:7b0d3c1e-0c39-4a43-9a0e-2f1f3a3e9d10",
            "jwp:v1:nuke:7b0d3c1e-0c39-4a43-9a0e-2f1f3a3e9d10",
        ] {
            assert_eq!(Id::parse(s), None, "{}", s);
        }
    }

    #[test]
    fn device_and_kick_choices() {
        assert_eq!(
            parse_device_choice(&encode_device_choice("abc123", true)),
            Some(("abc123".into(), true))
        );
        assert_eq!(parse_device_choice("r:abc"), Some(("abc".into(), false)));
        assert_eq!(parse_device_choice("x:abc"), None);
        assert_eq!(parse_device_choice("r:a/b"), None);
        assert_eq!(parse_device_choice("r:"), None);
        assert_eq!(
            parse_kick_choice("m:client-1"),
            Some((true, "client-1".into()))
        );
        assert_eq!(parse_kick_choice("u:123"), Some((false, "123".into())));
        assert_eq!(parse_kick_choice("u:abc"), None);
        assert_eq!(parse_kick_choice("z:1"), None);
    }
}
