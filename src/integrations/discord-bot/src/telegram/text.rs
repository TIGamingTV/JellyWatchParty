//! Telegram text: messages are HTML, and names from users are always
//! escaped, so they show as typed.

pub use crate::text::{join_within_by, status};

/// Telegram's limit for a message, in UTF-16 code units.
pub const MESSAGE_MAX: usize = 4096;

/// Length as Telegram counts it.
pub fn len16(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Escapes `s` for an HTML message (and drops control characters), cut
/// to `max` characters.
pub fn escape(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, c) in s.chars().filter(|c| !c.is_control()).enumerate() {
        if i == max {
            out.push('…');
            break;
        }
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// A button label: plain text, one line, at most 60 characters.
pub fn label(s: &str) -> String {
    let s: String = s.chars().filter(|c| !c.is_control()).collect();
    crate::text::fit(s.trim(), 60)
}

/// Joins items within `max` UTF-16 units (see `join_within`).
pub fn join_within(items: &[String], sep: &str, total: usize, max: usize) -> String {
    join_within_by(items, sep, total, max, len16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_is_escaped() {
        assert_eq!(
            escape("<b>Tom & Jerry</b>\n", 100),
            "&lt;b&gt;Tom &amp; Jerry&lt;/b&gt;"
        );
        assert_eq!(escape("abcdef", 3), "abc…");
        assert_eq!(escape("<<<<", 2), "&lt;&lt;…");
    }

    #[test]
    fn lengths_count_like_telegram() {
        assert_eq!(len16("abc"), 3);
        assert_eq!(len16("😀"), 2);
        assert_eq!(label(&"x".repeat(100)).chars().count(), 60);
        assert_eq!(label(" TV\n"), "TV");
    }
}
