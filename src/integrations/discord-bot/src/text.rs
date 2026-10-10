//! Text helpers: names from users are shown literally, never as markdown
//! or mentions.

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

/// Room member status in words.
pub fn status(s: &str) -> &'static str {
    match s {
        "synced" => "in sync",
        "playing" => "playing",
        "syncing" => "catching up",
        "buffering" => "buffering",
        "loading" => "loading",
        "paused" => "paused",
        "idle" => "not watching",
        "offline" => "offline",
        "error" => "problem",
        _ => "no status yet",
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

/// Cuts `s` to at most `max` characters, ending in `…` when cut.
pub fn fit(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars()
            .take(max.saturating_sub(1))
            .chain(std::iter::once('…'))
            .collect()
    }
}

/// Joins the first `items` with `sep` within `max` characters. If some
/// don't fit (or `total` says there are more than given), ends with
/// `and N more`. Items are never cut in half.
pub fn join_within(items: &[String], sep: &str, total: usize, max: usize) -> String {
    let total = total.max(items.len());
    let more = |n: usize| format!("and {} more", n);
    let len = |s: &str| s.chars().count();
    let mut out = String::new();
    let mut shown = 0;
    for item in items {
        let left = total - shown - 1;
        let sep_len = if shown == 0 { 0 } else { len(sep) };
        // Room for this item, plus the "and N more" that may follow it.
        let tail = if left > 0 {
            len(sep) + len(&more(left))
        } else {
            0
        };
        if len(&out) + sep_len + len(item) + tail > max {
            break;
        }
        if shown > 0 {
            out.push_str(sep);
        }
        out.push_str(item);
        shown += 1;
    }
    if shown < total {
        if shown > 0 {
            out.push_str(sep);
        }
        out.push_str(&more(total - shown));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_cuts_long_text() {
        assert_eq!(fit("abc", 3), "abc");
        assert_eq!(fit("abcd", 3), "ab…");
        assert_eq!(
            fit(&"é".repeat(3000), MESSAGE_MAX).chars().count(),
            MESSAGE_MAX
        );
    }

    #[test]
    fn join_within_keeps_whole_items_and_counts_the_rest() {
        let items: Vec<String> = (0..10).map(|i| format!("item{}", i)).collect();
        assert_eq!(join_within(&items[..2], ", ", 2, 100), "item0, item1");
        assert_eq!(
            join_within(&items[..2], ", ", 5, 100),
            "item0, item1, and 3 more"
        );
        let cut = join_within(&items, "\n", 10, 30);
        assert!(cut.chars().count() <= 30, "{}", cut);
        assert!(cut.ends_with("more"), "{}", cut);
        assert!(cut.starts_with("item0\nitem1"), "{}", cut);
        assert_eq!(join_within(&[], ", ", 0, 10), "");
        assert_eq!(join_within(&["x".repeat(50)], ", ", 1, 10), "and 1 more");
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
