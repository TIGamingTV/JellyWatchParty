//! The live room panel: one public message per room, kept up to date.
//! Rendering is pure (`render`) so it can be compared and tested; `embed`
//! and `components` turn it into Discord builders.

use super::ids::Id;
use super::text::{escape, join_within, mention, status};
use crate::api::Room;
use twilight_model::channel::message::component::{
    ActionRow, Button as DcButton, ButtonStyle, Component,
};
use twilight_model::channel::message::Embed;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder, EmbedFooterBuilder};

const COLOR_PLAYING: u32 = 0x3fb950;
const COLOR_IDLE: u32 = 0x00a4dc;
const COLOR_CLOSED: u32 = 0x6e7681;
const MAX_LISTED: usize = 15;
/// Discord's limit for an embed field's value.
const FIELD_MAX: usize = 1024;

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

/// Whether a panel says who controls playback: when the admin turned on
/// democratic mode commands (so owners see they can share control), or
/// when the room is democratic anyway (turned on from Jellyfin web).
pub fn shows_control(room: &Room, democratic_commands: bool) -> bool {
    democratic_commands || room.democratic
}

/// `democratic_commands`: the admin turned on democratic mode commands
/// (only then are there playback buttons).
pub fn render(room: &Room, democratic_commands: bool) -> PanelView {
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
            "\n\nNobody is watching yet. Join, then **Add my device**: add one as host, and everyone follows what it plays.",
        );
    }

    let host = match &room.host {
        Some(h) => escape(&h.name, 80),
        None => "Nobody yet".into(),
    };

    let members = if room.members.is_empty() {
        "-".to_string()
    } else {
        let lines: Vec<String> = room
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
        // Escaped names can be twice as long as typed: keep the field
        // within Discord's limit, or every edit of the panel is refused.
        join_within(&lines, "\n", room.members.len(), FIELD_MAX)
    };

    let people: Vec<String> = room
        .participants
        .iter()
        .take(MAX_LISTED)
        .map(|p| mention(p.external_id.as_deref(), &p.name))
        .collect();
    let people = join_within(&people, ", ", room.participants.len(), FIELD_MAX);

    let id = &room.id;
    let mut fields = vec![
        ("Host".into(), host),
        (format!("Watching ({})", room.members.len()), members),
        (
            format!("Joined on Discord ({})", room.participants.len()),
            if people.is_empty() {
                "-".into()
            } else {
                people
            },
        ),
    ];
    // After the others, so their places don't move.
    if shows_control(room, democratic_commands) {
        fields.push((
            "Control".into(),
            if room.democratic {
                "Everyone"
            } else {
                "Host only"
            }
            .into(),
        ));
    }
    let controls = room.democratic && democratic_commands;
    let footer = if controls {
        "Democratic mode: everyone in the room can play, pause and seek (buttons below or /jwp playback). Only your own Jellyfin devices can be added. Jellyfin web users join from their Watch Party panel."
    } else if democratic_commands {
        "Only your own Jellyfin devices can be added. The owner picks the host, can close the room and can turn on democratic mode (/jwp room democratic) so everyone can play, pause and seek. Jellyfin web users join from their Watch Party panel."
    } else {
        "Only your own Jellyfin devices can be added. The owner picks the host and can close the room. Jellyfin web users join from their Watch Party panel."
    };
    let mut rows = vec![
        vec![
            Button {
                id: Id::Join(id.clone()),
                label: "Join",
                style: Style::Primary,
            },
            Button {
                id: Id::AddDevice(id.clone()),
                label: "Add my device",
                style: Style::Success,
            },
            Button {
                id: Id::RemoveDevice(id.clone()),
                label: "Remove my device",
                style: Style::Secondary,
            },
            Button {
                id: Id::Leave(id.clone()),
                label: "Leave",
                style: Style::Secondary,
            },
        ],
        vec![
            Button {
                id: Id::PickHost(id.clone()),
                label: "Pick host",
                style: Style::Secondary,
            },
            Button {
                id: Id::Close(id.clone()),
                label: "Close room",
                style: Style::Danger,
            },
        ],
    ];
    if controls {
        rows.push(vec![
            Button {
                id: Id::Play(id.clone()),
                label: "Play",
                style: Style::Success,
            },
            Button {
                id: Id::Pause(id.clone()),
                label: "Pause",
                style: Style::Secondary,
            },
            Button {
                id: Id::Back(id.clone()),
                label: "−30s",
                style: Style::Secondary,
            },
            Button {
                id: Id::Forward(id.clone()),
                label: "+30s",
                style: Style::Secondary,
            },
        ]);
    }
    PanelView {
        title: format!("Watch party: {}", escape(&room.name, 100)),
        description,
        fields,
        footer: footer.into(),
        color: if room.play_state == "playing" && !room.members.is_empty() {
            COLOR_PLAYING
        } else {
            COLOR_IDLE
        },
        rows,
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
            serde_json::from_str(include_str!("../../../fixtures/rooms.json")).unwrap();
        r.rooms.into_iter().next().unwrap()
    }

    fn render(r: &Room) -> PanelView {
        super::render(r, false)
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
    fn a_full_room_with_awkward_names_still_fits() {
        // Every character of these names gets escaped, doubling their length.
        let awkward = "*_~`|>#[]()-:".repeat(10);
        let mut r = room();
        r.name = awkward.clone();
        r.owner.external_id = None;
        r.owner.name = awkward.clone();
        let member = r.members[1].clone();
        r.members = (0..20)
            .map(|i| crate::api::Member {
                id: format!("c{}", i),
                name: awkward.clone(),
                kind: "plugin_bridge".into(),
                status: "buffering".into(),
                ..member.clone()
            })
            .collect();
        let person = r.participants[0].clone();
        r.participants = (0..50)
            .map(|i| crate::api::Person {
                user_id: format!("u{}", i),
                name: awkward.clone(),
                external_id: None,
            })
            .chain(std::iter::once(person))
            .collect();
        twilight_validate::embed::embed(&embed(&super::render(&r, true))).unwrap();
        let v = render(&r);
        twilight_validate::embed::embed(&embed(&v)).unwrap();
        assert!(v.fields[1].1.ends_with("more"), "{}", v.fields[1].1);
        assert!(v.fields[2].1.ends_with("more"), "{}", v.fields[2].1);
    }

    #[test]
    fn democratic_rooms_get_playback_buttons_when_the_admin_allows() {
        let mut r = room();
        assert!(r.democratic);
        // Feature on, democratic room: a Control field and a third row.
        let v = super::render(&r, true);
        assert_eq!(v.fields[3], ("Control".into(), "Everyone".into()));
        assert_eq!(v.rows.len(), 3);
        let ids: Vec<String> = v.rows[2].iter().map(|b| b.id.encode()).collect();
        assert_eq!(
            ids,
            ["pb-play", "pb-pause", "pb-back", "pb-fwd"].map(|a| format!("jwp:v1:{}:{}", a, r.id))
        );
        assert!(v.footer.contains("Democratic mode"));
        assert!(v.rows.iter().flatten().all(|b| b.id.encode().len() <= 100));
        twilight_validate::embed::embed(&embed(&v)).unwrap();
        let c = components(&v);
        assert!(c
            .iter()
            .all(|r| matches!(r, Component::ActionRow(a) if a.components.len() <= 5)));

        // Feature off: no buttons, but the room says who controls it.
        let v = super::render(&r, false);
        assert_eq!(v.rows.len(), 2);
        assert_eq!(v.fields[3].1, "Everyone");
        assert!(!v.footer.contains("Democratic mode"));

        // Feature on, host-only room: says so, and how to change it.
        r.democratic = false;
        let v = super::render(&r, true);
        assert_eq!(v.rows.len(), 2);
        assert_eq!(v.fields[3].1, "Host only");
        assert!(v.footer.contains("/jwp room democratic"));

        // Neither: the panel is as it always was.
        let v = super::render(&r, false);
        assert_eq!(v.fields.len(), 3);
        assert_eq!(v.rows.len(), 2);

        // The switch alone changes the view (so panels are edited).
        assert_ne!(super::render(&r, true), super::render(&r, false));
    }

    #[test]
    fn closed_panels_have_no_buttons() {
        let v = render_closed("x", "This room is closed.");
        assert!(v.rows.is_empty());
        assert_eq!(v.color, COLOR_CLOSED);
    }
}
