//! The `/jwp` command tree and helpers to read its options.
//!
//! Passwords and link codes are never command options (those show up in
//! the channel as "used /jwp ..."); they are asked for in a modal.

use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::application_command::{
    CommandDataOption, CommandOptionValue,
};
use twilight_model::id::marker::UserMarker;
use twilight_model::id::Id;
use twilight_util::builder::command::{
    CommandBuilder, StringBuilder, SubCommandBuilder, SubCommandGroupBuilder, UserBuilder,
};

pub const NAME: &str = "jwp";

fn room_opt() -> StringBuilder {
    StringBuilder::new("room", "The room")
        .required(true)
        .autocomplete(true)
}

fn sub(name: &str, description: &str) -> SubCommandBuilder {
    SubCommandBuilder::new(name, description)
}

pub fn definition() -> Command {
    CommandBuilder::new(
        NAME,
        "Watch parties with your own Jellyfin devices",
        CommandType::ChatInput,
    )
    .option(sub(
        "link",
        "Link your Discord account to your Jellyfin account (asks for your name and code)",
    ))
    .option(sub("unlink", "Unlink your Discord account"))
    .option(sub(
        "whoami",
        "Show which Jellyfin account you're linked to and your rooms",
    ))
    .option(
        SubCommandGroupBuilder::new("room", "Watch party rooms").subcommands([
            sub(
                "create",
                "Create a room (asks for a name and an optional password)",
            ),
            sub("list", "List the rooms"),
            sub("join", "Join a room").option(room_opt()),
            sub("leave", "Leave a room (your devices leave too)").option(room_opt()),
            sub("host", "Owner: pick who everyone follows")
                .option(room_opt())
                .option(
                    StringBuilder::new("member", "The new host")
                        .required(true)
                        .autocomplete(true),
                ),
            sub("password", "Owner: set or remove the room password").option(room_opt()),
            sub("rename", "Owner: rename the room")
                .option(room_opt())
                .option(
                    StringBuilder::new("name", "New name")
                        .required(true)
                        .max_length(100),
                ),
            sub("transfer", "Owner: hand the room to another participant")
                .option(room_opt())
                .option(UserBuilder::new("user", "The new owner").required(true)),
            sub("kick", "Owner: remove a participant, device or viewer")
                .option(room_opt())
                .option(
                    StringBuilder::new("who", "Who to remove")
                        .required(true)
                        .autocomplete(true),
                ),
            sub("close", "Owner: close the room").option(room_opt()),
            sub("panel", "Owner: post the room's panel in this channel").option(room_opt()),
        ]),
    )
    .option(
        SubCommandGroupBuilder::new("device", "Your Jellyfin devices").subcommands([
            sub("add", "Put one of your devices into a room")
                .option(room_opt())
                .option(
                    StringBuilder::new("device", "Your device (open the Jellyfin app first)")
                        .required(true)
                        .autocomplete(true),
                )
                .option(
                    StringBuilder::new("role", "Follow the host, or be the host")
                        .required(true)
                        .choices([
                            ("receiver (follows the host)", "receiver"),
                            ("host (everyone follows it)", "host"),
                        ]),
                ),
            sub("remove", "Take one of your devices out of a room")
                .option(room_opt())
                .option(
                    StringBuilder::new("device", "Your device")
                        .required(true)
                        .autocomplete(true),
                ),
        ]),
    )
    .build()
}

/// A parsed invocation: the subcommand path (`["room", "join"]`) and its
/// options.
pub struct Invocation {
    pub path: Vec<String>,
    pub options: Vec<CommandDataOption>,
}

pub fn parse(options: Vec<CommandDataOption>) -> Invocation {
    let mut path = Vec::new();
    let mut current = options;
    while let Some(CommandDataOption {
        name,
        value: CommandOptionValue::SubCommandGroup(inner) | CommandOptionValue::SubCommand(inner),
    }) = current.first().cloned()
    {
        path.push(name);
        current = inner;
    }
    Invocation {
        path,
        options: current,
    }
}

impl Invocation {
    /// A string option (also the focused one in autocomplete).
    pub fn string(&self, name: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|o| o.name == name)
            .and_then(|o| match &o.value {
                CommandOptionValue::String(s) => Some(s.as_str()),
                CommandOptionValue::Focused(s, _) => Some(s.as_str()),
                _ => None,
            })
    }

    pub fn user(&self, name: &str) -> Option<Id<UserMarker>> {
        self.options
            .iter()
            .find(|o| o.name == name)
            .and_then(|o| match o.value {
                CommandOptionValue::User(u) => Some(u),
                _ => None,
            })
    }

    /// The option being typed in, for autocomplete.
    pub fn focused(&self) -> Option<(&str, &str)> {
        self.options.iter().find_map(|o| match &o.value {
            CommandOptionValue::Focused(v, _) => Some((o.name.as_str(), v.as_str())),
            _ => None,
        })
    }

    pub fn is(&self, path: &[&str]) -> bool {
        self.path.len() == path.len() && self.path.iter().zip(path).all(|(a, b)| a == b)
    }

    pub fn path(&self) -> Vec<&str> {
        self.path.iter().map(String::as_str).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use twilight_model::application::command::CommandOptionType;

    #[test]
    fn the_definition_is_valid_for_discord() {
        let cmd = definition();
        twilight_validate::command::command(&cmd).unwrap();
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(json["name"], "jwp");
        fn check(v: &serde_json::Value) {
            if let Some(name) = v["name"].as_str() {
                assert!(
                    name.len() <= 32
                        && name
                            .chars()
                            .all(|c| c.is_ascii_lowercase() || c == '-' || c == '_'),
                    "{}",
                    name
                );
            }
            if let Some(d) = v["description"].as_str() {
                assert!(!d.is_empty() && d.chars().count() <= 100, "{}", d);
            }
            if let Some(opts) = v["options"].as_array() {
                assert!(opts.len() <= 25);
                for o in opts {
                    check(o);
                }
            }
        }
        check(&json);
    }

    #[test]
    fn invocations_are_parsed_down_to_their_subcommand() {
        let inv = parse(vec![CommandDataOption {
            name: "room".into(),
            value: CommandOptionValue::SubCommandGroup(vec![CommandDataOption {
                name: "host".into(),
                value: CommandOptionValue::SubCommand(vec![
                    CommandDataOption {
                        name: "room".into(),
                        value: CommandOptionValue::String("r1".into()),
                    },
                    CommandDataOption {
                        name: "member".into(),
                        value: CommandOptionValue::Focused("ali".into(), CommandOptionType::String),
                    },
                ]),
            }]),
        }]);
        assert!(inv.is(&["room", "host"]));
        assert!(!inv.is(&["room"]));
        assert_eq!(inv.string("room"), Some("r1"));
        assert_eq!(inv.focused(), Some(("member", "ali")));
        assert_eq!(inv.string("nope"), None);

        let inv = parse(vec![CommandDataOption {
            name: "link".into(),
            value: CommandOptionValue::SubCommand(vec![]),
        }]);
        assert_eq!(inv.path(), vec!["link"]);
    }
}
