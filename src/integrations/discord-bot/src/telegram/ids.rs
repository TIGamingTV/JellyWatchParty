//! What Telegram carries for us: button data (at most 64 bytes) and
//! `/start` payloads from `t.me/<bot>?start=...` links (at most 64 of
//! `A-Za-z0-9_-`). Choices too long for a button (a device's session id
//! and the room) are kept here as short-lived "picks".
//!
//! Anyone can send any data to the bot, so these are only hints about what
//! was pressed: the server checks every action again.

use crate::api::valid_room_id;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const PREFIX: &str = "v1:";
pub const DATA_MAX: usize = 64;
const PICK_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_PICKS: usize = 5000;

/// A button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Data {
    // Panel (and private room menu) buttons.
    Join(String),
    Leave(String),
    AddDevice(String),
    RemoveDevice(String),
    Manage(String),
    // Private menus.
    Room(String),
    Rename(String),
    Password(String),
    HostMenu(String),
    KickMenu(String),
    TransferMenu(String),
    Close(String),
    ConfirmClose(String),
    Repost(String),
    Pick(String),
    Cancel,
}

fn valid_token(t: &str) -> bool {
    !t.is_empty() && t.len() <= 16 && t.bytes().all(|b| b.is_ascii_alphanumeric())
}

impl Data {
    pub fn encode(&self) -> String {
        let (action, arg) = match self {
            Data::Join(r) => ("join", Some(r)),
            Data::Leave(r) => ("leave", Some(r)),
            Data::AddDevice(r) => ("dev", Some(r)),
            Data::RemoveDevice(r) => ("undev", Some(r)),
            Data::Manage(r) => ("mng", Some(r)),
            Data::Room(r) => ("room", Some(r)),
            Data::Rename(r) => ("ren", Some(r)),
            Data::Password(r) => ("pw", Some(r)),
            Data::HostMenu(r) => ("host", Some(r)),
            Data::KickMenu(r) => ("kick", Some(r)),
            Data::TransferMenu(r) => ("xfer", Some(r)),
            Data::Close(r) => ("close", Some(r)),
            Data::ConfirmClose(r) => ("closeok", Some(r)),
            Data::Repost(r) => ("repost", Some(r)),
            Data::Pick(t) => ("pk", Some(t)),
            Data::Cancel => ("cancel", None),
        };
        match arg {
            Some(a) => format!("{}{}:{}", PREFIX, action, a),
            None => format!("{}{}", PREFIX, action),
        }
    }

    pub fn parse(s: &str) -> Option<Data> {
        if s.len() > DATA_MAX {
            return None;
        }
        let rest = s.strip_prefix(PREFIX)?;
        let (action, arg) = match rest.split_once(':') {
            Some((a, r)) => (a, Some(r)),
            None => (rest, None),
        };
        if action == "pk" {
            return arg.filter(|t| valid_token(t)).map(|t| Data::Pick(t.into()));
        }
        if action == "cancel" {
            return arg.is_none().then_some(Data::Cancel);
        }
        let r = arg.filter(|r| valid_room_id(r))?.to_string();
        Some(match action {
            "join" => Data::Join(r),
            "leave" => Data::Leave(r),
            "dev" => Data::AddDevice(r),
            "undev" => Data::RemoveDevice(r),
            "mng" => Data::Manage(r),
            "room" => Data::Room(r),
            "ren" => Data::Rename(r),
            "pw" => Data::Password(r),
            "host" => Data::HostMenu(r),
            "kick" => Data::KickMenu(r),
            "xfer" => Data::TransferMenu(r),
            "close" => Data::Close(r),
            "closeok" => Data::ConfirmClose(r),
            "repost" => Data::Repost(r),
            _ => return None,
        })
    }
}

/// What a `/start` link opens in the private chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    Link,
    /// Create a room; its panel goes to this forum topic of the group.
    New(Option<i64>),
    Join(String),
    AddDevice(String),
    RemoveDevice(String),
    Manage(String),
}

impl Start {
    pub fn encode(&self) -> String {
        match self {
            Start::Link => "link".into(),
            Start::New(None) => "new".into(),
            Start::New(Some(t)) => format!("new-{}", t),
            Start::Join(r) => format!("join-{}", r),
            Start::AddDevice(r) => format!("dev-{}", r),
            Start::RemoveDevice(r) => format!("undev-{}", r),
            Start::Manage(r) => format!("mng-{}", r),
        }
    }

    pub fn parse(s: &str) -> Option<Start> {
        let s = s.trim();
        if s.len() > 64 {
            return None;
        }
        let (what, arg) = match s.split_once('-') {
            Some((w, a)) => (w, Some(a)),
            None => (s, None),
        };
        let room = || arg.filter(|r| valid_room_id(r)).map(str::to_string);
        Some(match (what, arg) {
            ("link", None) => Start::Link,
            ("new", None) => Start::New(None),
            ("new", Some(t)) => Start::New(Some(t.parse().ok().filter(|t: &i64| *t > 0)?)),
            ("join", Some(_)) => Start::Join(room()?),
            ("dev", Some(_)) => Start::AddDevice(room()?),
            ("undev", Some(_)) => Start::RemoveDevice(room()?),
            ("mng", Some(_)) => Start::Manage(room()?),
            _ => return None,
        })
    }

    /// The link that opens this in a private chat with the bot.
    pub fn link(&self, bot_username: &str) -> String {
        format!("https://t.me/{}?start={}", bot_username, self.encode())
    }
}

/// A choice made in a private menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    AddDevice {
        room: String,
        session: String,
        host: bool,
    },
    RemoveDevice {
        room: String,
        member: String,
    },
    Host {
        room: String,
        member: String,
    },
    /// A member (device or viewer), or a participant by Telegram id.
    Kick {
        room: String,
        member: Option<String>,
        user: Option<String>,
    },
    Transfer {
        room: String,
        to: String,
    },
}

/// Picks offered to users, by token. A pick only works for the user it
/// was offered to.
#[derive(Debug, Default)]
pub struct Picks {
    next: u64,
    offered: HashMap<String, (i64, Instant, Pick)>,
}

impl Picks {
    pub fn offer(&mut self, user: i64, pick: Pick) -> Data {
        let now = Instant::now();
        if self.offered.len() >= MAX_PICKS {
            self.offered
                .retain(|_, (_, at, _)| now.duration_since(*at) < PICK_TTL);
        }
        self.next += 1;
        let token = format!("{:x}", self.next);
        self.offered.insert(token.clone(), (user, now, pick));
        Data::Pick(token)
    }

    pub fn get(&self, token: &str, user: i64) -> Option<Pick> {
        self.offered
            .get(token)
            .filter(|(u, at, _)| *u == user && at.elapsed() < PICK_TTL)
            .map(|(_, _, p)| p.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: &str = "7b0d3c1e-0c39-4a43-9a0e-2f1f3a3e9d10";

    #[test]
    fn data_round_trips_within_telegrams_limit() {
        let r = || ROOM.to_string();
        for d in [
            Data::Join(r()),
            Data::Leave(r()),
            Data::AddDevice(r()),
            Data::RemoveDevice(r()),
            Data::Manage(r()),
            Data::Room(r()),
            Data::Rename(r()),
            Data::Password(r()),
            Data::HostMenu(r()),
            Data::KickMenu(r()),
            Data::TransferMenu(r()),
            Data::Close(r()),
            Data::ConfirmClose(r()),
            Data::Repost(r()),
            Data::Pick("ffffffffffffffff".into()),
            Data::Cancel,
        ] {
            let s = d.encode();
            assert!(s.len() <= DATA_MAX, "{}", s);
            assert_eq!(Data::parse(&s), Some(d));
        }
    }

    #[test]
    fn forged_data_is_ignored() {
        for s in [
            "",
            "v1:",
            "v2:join:7b0d3c1e-0c39-4a43-9a0e-2f1f3a3e9d10",
            "v1:join",
            "v1:join:../admin",
            "v1:nuke:7b0d3c1e-0c39-4a43-9a0e-2f1f3a3e9d10",
            "v1:pk:",
            "v1:pk:a/b",
            "v1:cancel:x",
        ] {
            assert_eq!(Data::parse(s), None, "{}", s);
        }
    }

    #[test]
    fn start_payloads_round_trip_and_fit() {
        let r = || ROOM.to_string();
        for s in [
            Start::Link,
            Start::New(None),
            Start::New(Some(1234)),
            Start::Join(r()),
            Start::AddDevice(r()),
            Start::RemoveDevice(r()),
            Start::Manage(r()),
        ] {
            let e = s.encode();
            assert!(e.len() <= 64, "{}", e);
            assert!(
                e.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
                "{}",
                e
            );
            assert_eq!(Start::parse(&e), Some(s));
        }
        for bad in [
            "", "new-x", "new--1", "join", "join-abc", "link-1", "mng-../x",
        ] {
            assert_eq!(Start::parse(bad), None, "{}", bad);
        }
        assert_eq!(Start::Link.link("JwpBot"), "https://t.me/JwpBot?start=link");
    }

    #[test]
    fn picks_belong_to_their_user() {
        let mut p = Picks::default();
        let pick = Pick::Host {
            room: ROOM.into(),
            member: "c1".into(),
        };
        let Data::Pick(t) = p.offer(5, pick.clone()) else {
            panic!("a pick")
        };
        assert_eq!(p.get(&t, 5), Some(pick));
        assert_eq!(p.get(&t, 6), None);
        assert_eq!(p.get("zz", 5), None);
        let Data::Pick(t2) = p.offer(
            5,
            Pick::Transfer {
                room: ROOM.into(),
                to: "9".into(),
            },
        ) else {
            panic!("a pick")
        };
        assert_ne!(t, t2);
    }
}
