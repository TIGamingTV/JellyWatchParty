//! The live room panel: one public message per room, kept up to date.
//! Rendering is pure (`render`) so it can be compared and tested; `embed`
//! and `components` turn it into Discord builders.

use crate::api::Room;
use crate::ids::Id;
use crate::text::{escape, mention, status};
use twilight_model::channel::message::component::{
    ActionRow, Button as DcButton, ButtonStyle, Component,
};
use twilight_model::channel::message::Embed;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder, EmbedFooterBuilder};

const COLOR_PLAYING: u32 = 0x3fb950;
const COLOR_IDLE: u32 = 0x00a4dc;
const COLOR_CLOSED: u32 = 0x6e7681;
const MAX_LISTED: usize = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Primary,
    Secondary,
    Success,
    Danger,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub id: Id,
    pub label: &'static str,
    pub style: Style,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelView {
    pub title: String,
    pub description: String,
    pub fields: Vec<(String, String)>,
    pub footer: String,
    pub color: u32,
    pub rows: Vec<Vec<Button>>,
}

fn kind_label(kind: &str) -> &'static str {
    match kind {
        "jellyfin" => "device",
        "plugin_bridge" => "device (panel bridge)",
        _ => "Watch Party panel",
    }
}

pub fn render(room: &Room) -> PanelView {
    let mut description = format!(
        "Owner: {}\n{}",
        mention(room.owner.external_id.as_deref(), &room.owner.name),
        if room.has_password {
            "Password protected: join with the password from the owner."
        } else {
            "Open: anyone may join."
        }
    );
    if room.members.is_empty() {
        description.push_str(
            "\n\nNobody is watching yet. Join, then **Add my device**: the first device becomes the host everyone follows.",
        );
    }

    let host = match &room.host {
        Some(h) => escape(&h.name, 80),
        None => "Nobody yet".into(),
    };

    let members = if room.members.is_empty() {
        "-".to_string()
    } else {
        let mut lines: Vec<String> = room
            .members
            .iter()
            .take(MAX_LISTED)
            .map(|m| {
                format!(
                    "{} {} - {}{}",
                    if m.is_host { "**Host**" } else { "Receiver" },
                    escape(&m.name, 60),
                    status(&m.status),
                    if m.kind == "web" {
                        String::new()
                    } else {
                        format!(" ({})", kind_label(&m.kind))
                    }
                )
            })
            .collect();
        if room.members.len() > MAX_LISTED {
            lines.push(format!("and {} more", room.members.len() - MAX_LISTED));
        }
        lines.join("\n")
    };

    let mut people: Vec<String> = room
        .participants
        .iter()
        .take(MAX_LISTED)
        .map(|p| mention(p.external_id.as_deref(), &p.name))
        .collect();
    if room.participants.len() > MAX_LISTED {
        people.push(format!("and {} more", room.participants.len() - MAX_LISTED));
    }

    let id = &room.id;
    PanelView {
        title: format!("Watch party: {}", escape(&room.name, 100)),
        description,
        fields: vec![
            ("Host".into(), host),
            (format!("Watching ({})", room.members.len()), members),
            (
                format!("Joined on Discord ({})", room.participants.len()),
                if people.is_empty() { "-".into() } else { people.join(", ") },
            ),
        ],
        footer: "Only your own Jellyfin devices can be added. The owner picks the host and can close the room. Jellyfin web users join from their Watch Party panel.".into(),
        color: if room.play_state == "playing" && !room.members.is_empty() {
            COLOR_PLAYING
        } else {
            COLOR_IDLE
        },
        rows: vec![
            vec![
                Button { id: Id::Join(id.clone()), label: "Join", style: Style::Primary },
                Button { id: Id::AddDevice(id.clone()), label: "Add my device", style: Style::Success },
                Button { id: Id::RemoveDevice(id.clone()), label: "Remove my device", style: Style::Secondary },
                Button { id: Id::Leave(id.clone()), label: "Leave", style: Style::Secondary },
            ],
            vec![
                Button { id: Id::PickHost(id.clone()), label: "Pick host", style: Style::Secondary },
                Button { id: Id::Close(id.clone()), label: "Close room", style: Style::Danger },
            ],
        ],
    }
}

/// What a panel shows once its room is gone (or the panel moved).
pub fn render_closed(name: &str, why: &str) -> PanelView {
    PanelView {
        title: format!("Watch party: {}", escape(name, 100)),
        description: why.to_string(),
        fields: Vec::new(),
        footer: "Start a new one with /jwp room create.".into(),
        color: COLOR_CLOSED,
        rows: Vec::new(),
    }
}

pub fn embed(v: &PanelView) -> Embed {
    let mut e = EmbedBuilder::new()
        .title(&v.title)
        .description(&v.description)
        .color(v.color)
        .footer(EmbedFooterBuilder::new(&v.footer));
    for (name, value) in &v.fields {
        e = e.field(EmbedFieldBuilder::new(name, value));
    }
    e.build()
}

pub fn button(id: &Id, label: &str, style: Style) -> Component {
    Component::Button(DcButton {
        custom_id: Some(id.encode()),
        disabled: false,
        emoji: None,
        label: Some(label.to_string()),
        style: match style {
            Style::Primary => ButtonStyle::Primary,
            Style::Secondary => ButtonStyle::Secondary,
            Style::Success => ButtonStyle::Success,
            Style::Danger => ButtonStyle::Danger,
        },
        url: None,
        sku_id: None,
    })
}

pub fn components(v: &PanelView) -> Vec<Component> {
    v.rows
        .iter()
        .map(|row| {
            Component::ActionRow(ActionRow {
                components: row
                    .iter()
                    .map(|b| button(&b.id, b.label, b.style))
                    .collect(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::RoomsResponse;

    fn room() -> Room {
        let r: RoomsResponse =
            serde_json::from_str(include_str!("../../fixtures/rooms.json")).unwrap();
        r.rooms.into_iter().next().unwrap()
    }

    #[test]
    fn renders_owner_host_members_and_participants() {
        let v = render(&room());
        assert_eq!(v.title, "Watch party: Friday movie night");
        assert!(v.description.contains("<@100000000000000001>"));
        assert!(v.description.contains("Password protected"));
        assert_eq!(v.fields[0].1, "Alice \\(Living room TV\\)");
        assert!(v.fields[1].1.contains("**Host** Alice"));
        assert!(v.fields[1].1.contains("(device)"));
        assert!(v.fields[1].1.contains("Receiver Bob - in sync"));
        // Bob has no linked account: shown by name.
        assert!(v.fields[2].1.contains("Bob"));
        assert_eq!(v.color, COLOR_PLAYING);
        assert_eq!(v.rows.len(), 2);
        assert!(v.rows.iter().flatten().all(|b| b.id.encode().len() <= 100));
    }

    #[test]
    fn names_cannot_inject_markdown_or_pings() {
        let mut r = room();
        r.name = "@everyone **free nitro**".into();
        r.members[1].name = "[click](https://evil)".into();
        let v = render(&r);
        assert!(!v.title.contains("@everyone"));
        assert!(!v.title.contains("**free"));
        assert!(!v.fields[1].1.contains("[click]("));
    }

    #[test]
    fn an_empty_room_explains_what_to_do() {
        let mut r = room();
        r.members.clear();
        r.host = None;
        let v = render(&r);
        assert!(v.description.contains("Add my device"));
        assert_eq!(v.fields[0].1, "Nobody yet");
        assert_eq!(v.color, COLOR_IDLE);
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
    fn the_embed_fits_discords_limits() {
        let v = render(&room());
        let e = embed(&v);
        assert!(e.title.as_ref().unwrap().chars().count() <= 256);
        assert!(e.description.as_ref().unwrap().chars().count() <= 4096);
        assert!(e
            .fields
            .iter()
            .all(|f| f.name.chars().count() <= 256 && f.value.chars().count() <= 1024));
        assert!(e.footer.as_ref().unwrap().text.chars().count() <= 2048);
        let c = components(&v);
        assert!(c.len() <= 5);
        assert!(c
            .iter()
            .all(|r| matches!(r, Component::ActionRow(a) if a.components.len() <= 5)));
    }

    #[test]
    fn closed_panels_have_no_buttons() {
        let v = render_closed("x", "This room is closed.");
        assert!(v.rows.is_empty());
        assert_eq!(v.color, COLOR_CLOSED);
    }
}
