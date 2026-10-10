//! The live room panel on Telegram: one HTML message per room in the
//! group, with buttons. Rendering is pure so views can be compared.

use super::ids::Data;
use super::text::{escape, join_within, label, status};
use crate::api::Room;
use serde_json::{json, Value};

const MAX_LISTED: usize = 15;
/// Budgets (UTF-16 units) that keep a full panel within one message.
const MEMBERS_MAX: usize = 1400;
const PEOPLE_MAX: usize = 900;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Button {
    Data(String, Data),
    Url(String, String),
}

impl Button {
    pub fn data(text: &str, data: Data) -> Self {
        Button::Data(label(text), data)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    /// HTML.
    pub text: String,
    pub rows: Vec<Vec<Button>>,
}

/// An inline keyboard (an empty one removes the buttons).
pub fn keyboard(rows: &[Vec<Button>]) -> Value {
    let rows: Vec<Vec<Value>> = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|b| match b {
                    Button::Data(text, data) => {
                        json!({ "text": text, "callback_data": data.encode() })
                    }
                    Button::Url(text, url) => json!({ "text": text, "url": url }),
                })
                .collect()
        })
        .collect();
    json!({ "inline_keyboard": rows })
}

fn kind_label(kind: &str) -> &'static str {
    match kind {
        "jellyfin" => " (device)",
        "plugin_bridge" => " (device, panel bridge)",
        _ => "",
    }
}

/// The buttons of a room, on its panel and in private room menus.
pub fn room_buttons(id: &str) -> Vec<Vec<Button>> {
    let id = id.to_string();
    vec![
        vec![
            Button::data("Join", Data::Join(id.clone())),
            Button::data("Leave", Data::Leave(id.clone())),
        ],
        vec![
            Button::data("Add my device", Data::AddDevice(id.clone())),
            Button::data("Remove my device", Data::RemoveDevice(id.clone())),
        ],
        vec![Button::data("Manage (owner)", Data::Manage(id))],
    ]
}

pub fn render(room: &Room) -> View {
    let mut text = format!(
        "<b>Watch party: {}</b>\nOwner: {}\n{}",
        escape(&room.name, 100),
        escape(&room.owner.name, 60),
        if room.has_password {
            "Password protected: join with the password from the owner."
        } else {
            "Open: anyone may join."
        }
    );
    if room.members.is_empty() {
        text.push_str(
            "\n\nNobody is watching yet. <b>Join</b>, then <b>Add my device</b>: add one as host, and everyone follows what it plays.",
        );
    } else if room.play_state == "playing" {
        text.push_str("\n\nPlaying now.");
    }

    let host = match &room.host {
        Some(h) => escape(&h.name, 80),
        None => "nobody yet".into(),
    };
    text.push_str(&format!("\n\n<b>Host:</b> {}", host));

    text.push_str(&format!("\n<b>Watching ({}):</b>", room.members.len()));
    if room.members.is_empty() {
        text.push_str(" -");
    } else {
        let lines: Vec<String> = room
            .members
            .iter()
            .take(MAX_LISTED)
            .map(|m| {
                format!(
                    "• {} {} - {}{}",
                    if m.is_host { "<b>Host</b>" } else { "Receiver" },
                    escape(&m.name, 60),
                    status(&m.status),
                    kind_label(&m.kind)
                )
            })
            .collect();
        text.push('\n');
        text.push_str(&join_within(&lines, "\n", room.members.len(), MEMBERS_MAX));
    }

    let people: Vec<String> = room
        .participants
        .iter()
        .take(MAX_LISTED)
        .map(|p| escape(&p.name, 60))
        .collect();
    let people = join_within(&people, ", ", room.participants.len(), PEOPLE_MAX);
    text.push_str(&format!(
        "\n<b>Joined on Telegram ({}):</b> {}",
        room.participants.len(),
        if people.is_empty() {
            "-".into()
        } else {
            people
        }
    ));
    text.push_str(
        "\n\n<i>Only your own Jellyfin devices can be added. The owner picks the host and can close the room. Jellyfin web users join from their Watch Party panel.</i>",
    );

    View {
        text,
        rows: room_buttons(&room.id),
    }
}

/// What a panel shows once its room is gone (or the panel moved).
pub fn render_closed(name: &str, why: &str) -> View {
    View {
        text: format!(
            "<b>Watch party: {}</b>\n{}\n\n<i>Start a new one with /newroom.</i>",
            escape(name, 100),
            escape(why, 200)
        ),
        rows: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Member, Person, RoomsResponse};
    use crate::telegram::ids::DATA_MAX;
    use crate::telegram::text::{len16, MESSAGE_MAX};

    fn room() -> Room {
        let r: RoomsResponse =
            serde_json::from_str(include_str!("../../../fixtures/rooms.json")).unwrap();
        r.rooms.into_iter().next().unwrap()
    }

    #[test]
    fn renders_owner_host_members_and_participants() {
        let v = render(&room());
        assert!(v.text.starts_with("<b>Watch party: Friday movie night</b>"));
        assert!(v.text.contains("Owner: Alice"));
        assert!(v.text.contains("Password protected"));
        assert!(v.text.contains("<b>Host:</b> Alice (Living room TV)"));
        assert!(v
            .text
            .contains("• <b>Host</b> Alice (Living room TV) - playing (device)"));
        assert!(v.text.contains("• Receiver Bob - in sync"));
        assert!(v.text.contains("<b>Joined on Telegram (2):</b> Alice, Bob"));
        assert!(v.text.contains("Playing now."));
        assert_eq!(v.rows.len(), 3);
        for b in v.rows.iter().flatten() {
            let Button::Data(_, d) = b else {
                panic!("panel buttons carry data")
            };
            assert!(d.encode().len() <= DATA_MAX);
        }
    }

    #[test]
    fn names_cannot_inject_html() {
        let mut r = room();
        r.name = "<a href=\"https://evil\">free</a>".into();
        r.members[1].name = "</b><i>x".into();
        let v = render(&r);
        assert!(!v.text.contains("<a "));
        assert!(!v.text.contains("</b><i>"));
        assert!(v.text.contains("&lt;a href=\"https://evil\"&gt;"));
    }

    #[test]
    fn an_empty_room_explains_what_to_do() {
        let mut r = room();
        r.members.clear();
        r.host = None;
        let v = render(&r);
        assert!(v.text.contains("Add my device"));
        assert!(v.text.contains("<b>Host:</b> nobody yet"));
        assert!(!v.text.contains("Playing now"));
    }

    #[test]
    fn a_change_changes_the_view() {
        let a = render(&room());
        let mut r = room();
        r.members[1].status = "syncing".into();
        assert_ne!(a, render(&r));
        assert_eq!(a, render(&room()));
    }

    #[test]
    fn a_full_room_with_awkward_names_still_fits() {
        for awkward in ["&<>".repeat(40), "😀".repeat(120)] {
            let mut r = room();
            r.name = awkward.clone();
            r.owner.name = awkward.clone();
            r.host.as_mut().unwrap().name = awkward.clone();
            let member = r.members[1].clone();
            r.members = (0..20)
                .map(|i| Member {
                    id: format!("c{}", i),
                    name: awkward.clone(),
                    kind: "plugin_bridge".into(),
                    status: "buffering".into(),
                    ..member.clone()
                })
                .collect();
            r.participants = (0..50)
                .map(|i| Person {
                    user_id: format!("u{}", i),
                    name: awkward.clone(),
                    external_id: None,
                })
                .collect();
            let v = render(&r);
            assert!(len16(&v.text) <= MESSAGE_MAX, "{}", len16(&v.text));
            assert!(v.text.contains("more"));
        }
        let c = render_closed(&"&".repeat(200), crate::panels::CLOSED);
        assert!(len16(&c.text) <= MESSAGE_MAX);
        assert!(c.rows.is_empty());
    }

    #[test]
    fn keyboards_are_telegram_shaped() {
        let k = keyboard(&[vec![
            Button::data("Join", Data::Cancel),
            Button::Url("Open".into(), "https://t.me/x?start=link".into()),
        ]]);
        assert_eq!(k["inline_keyboard"][0][0]["callback_data"], "v1:cancel");
        assert_eq!(
            k["inline_keyboard"][0][1]["url"],
            "https://t.me/x?start=link"
        );
        assert_eq!(keyboard(&[]), json!({ "inline_keyboard": [] }));
    }
}
