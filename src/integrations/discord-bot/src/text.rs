//! Text helpers every platform uses. How names are escaped is up to each
//! platform (Discord markdown, Telegram HTML).

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
    join_within_by(items, sep, total, max, |s| s.chars().count())
}

/// `join_within`, measuring length with `len`.
pub fn join_within_by(
    items: &[String],
    sep: &str,
    total: usize,
    max: usize,
    len: impl Fn(&str) -> usize,
) -> String {
    let total = total.max(items.len());
    let more = |n: usize| format!("and {} more", n);
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
        assert_eq!(fit(&"é".repeat(3000), 2000).chars().count(), 2000);
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
    fn join_within_by_measures_its_own_way() {
        // Emoji are two UTF-16 units.
        let items = vec!["😀😀".to_string(), "😀😀".to_string()];
        let utf16 = |s: &str| s.encode_utf16().count();
        assert_eq!(join_within_by(&items, ",", 2, 14, utf16), "and 2 more");
        assert_eq!(join_within(&items, ",", 2, 14), "😀😀,😀😀");
    }
}
