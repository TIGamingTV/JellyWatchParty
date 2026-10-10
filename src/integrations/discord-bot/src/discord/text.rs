//! Discord text: names from users are shown literally, never as markdown
//! or mentions.

pub use crate::text::{fit, join_within, status};

/// Escapes Discord markdown and breaks up mentions, so a room or device
/// name shows as typed. Also trims to `max` characters.
pub fn escape(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, c) in s.chars().filter(|c| !c.is_control()).enumerate() {
        if i == max {
            out.push('…');
            break;
        }
        match c {
            '\\' | '*' | '_' | '~' | '`' | '|' | '>' | '#' | '[' | ']' | '(' | ')' | '-' | ':' => {
                out.push('\\');
                out.push(c);
            }
            // A zero-width space after @ keeps "@everyone" and friends inert
            // even where allowed_mentions isn't applied (embeds, logs).
            '@' => out.push_str("@\u{200b}"),
            '<' => out.push_str("<\u{200b}"),
            _ => out.push(c),
        }
    }
    out
}

/// A user mention (doesn't ping inside embeds, and every message from this
/// bot sends with no allowed mentions).
pub fn mention(external_id: Option<&str>, fallback: &str) -> String {
    match external_id {
        Some(id) if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) => {
            format!("<@{}>", id)
        }
        _ => escape(fallback, 60),
    }
}

/// Cuts a label for a select option / autocomplete choice (max 100).
pub fn label(s: &str) -> String {
    let s: String = s.chars().filter(|c| !c.is_control()).collect();
    if s.chars().count() <= 100 {
        s
    } else {
        s.chars().take(99).chain(std::iter::once('…')).collect()
    }
}

/// Discord's limit for a message's text.
pub const MESSAGE_MAX: usize = 2000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_fit_discords_limit() {
        assert_eq!(
            fit(&"é".repeat(3000), MESSAGE_MAX).chars().count(),
            MESSAGE_MAX
        );
    }

    #[test]
    fn markdown_and_mentions_are_neutralised() {
        assert_eq!(
            escape("**bold** @everyone", 100),
            "\\*\\*bold\\*\\* @\u{200b}everyone"
        );
        assert_eq!(escape("<@123>", 100), "<\u{200b}@\u{200b}123\\>");
        assert_eq!(escape("a\nb", 100), "ab");
        assert_eq!(escape("abcdef", 3), "abc…");
    }

    #[test]
    fn mentions_only_for_numeric_ids() {
        assert_eq!(mention(Some("123"), "x"), "<@123>");
        assert_eq!(mention(Some("12a"), "Bob_"), "Bob\\_");
        assert_eq!(mention(None, "Bob"), "Bob");
    }

    #[test]
    fn labels_fit_discord() {
        assert_eq!(label(&"x".repeat(150)).chars().count(), 100);
        assert_eq!(label("TV"), "TV");
    }
}
