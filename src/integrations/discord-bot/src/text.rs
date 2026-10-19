//! Text helpers every platform uses. How names are escaped is up to each
//! platform (Discord markdown, Telegram HTML).

use crate::api::Playback;
use serde_json::Value;

/// How far the −30s / +30s buttons jump (seconds).
pub const SKIP_SECS: i64 = 30;

/// Where to seek: to a position, or by an amount (seconds).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seek {
    To(u64),
    By(i64),
}

impl From<Seek> for Playback {
    fn from(s: Seek) -> Self {
        match s {
            Seek::To(p) => Playback::SeekTo(p),
            Seek::By(d) => Playback::SeekBy(d),
        }
    }
}

/// A time as typed: `1:23:45`, `83:10` or `95` (seconds) go there;
/// `+30`, `-10` or `+1:30` jump ahead or back by that much.
pub fn parse_seek(s: &str) -> Option<Seek> {
    let s = s.trim();
    let (sign, clock) = match s.chars().next()? {
        '+' => (Some(1), &s[1..]),
        '-' => (Some(-1), &s[1..]),
        // A typographic minus, as phones like to type it.
        '−' => (Some(-1), &s['−'.len_utf8()..]),
        _ => (None, s),
    };
    let parts: Vec<&str> = clock.split(':').collect();
    if parts.len() > 3 {
        return None;
    }
    let mut secs: u64 = 0;
    for (i, p) in parts.iter().enumerate() {
        // The first part may be any size (`83:10`, `95`); later ones are
        // minutes or seconds.
        let ok = !p.is_empty()
            && p.bytes().all(|b| b.is_ascii_digit())
            && if i == 0 { p.len() <= 6 } else { p.len() <= 2 };
        if !ok {
            return None;
        }
        let n: u64 = p.parse().ok()?;
        if i > 0 && n >= 60 {
            return None;
        }
        secs = secs * 60 + n;
    }
    Some(match sign {
        None => Seek::To(secs),
        Some(k) => Seek::By(k * i64::try_from(secs).ok()?),
    })
}

/// `h:mm:ss` / `m:ss`.
pub fn clock(secs: f64) -> String {
    let s = if secs.is_finite() {
        secs.max(0.0) as u64
    } else {
        0
    };
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, s)
    } else {
        format!("{}:{:02}", m, s)
    }
}

/// What turning democratic mode on or off did.
pub fn democratic_done(on: bool) -> &'static str {
    if on {
        "Democratic mode is on: everyone in the room can play, pause and seek."
    } else {
        "Democratic mode is off: only the host controls playback."
    }
}

/// How to write a time to seek to.
pub const SEEK_HELP: &str =
    "Give a time like 1:23:45, 83:10 or 95 (seconds) to go there, or +30 / -10 to jump ahead or back.";

/// What a playback request did, from the server's answer
/// (`{action, position}`).
pub fn playback_done(answer: &Value) -> String {
    match answer["action"].as_str() {
        Some("play") => "Playing.".into(),
        Some("pause") => "Paused.".into(),
        Some("seek") => match answer["position"].as_f64() {
            Some(p) => format!("Jumped to {}.", clock(p)),
            None => "Jumped.".into(),
        },
        _ => "Done.".into(),
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
    fn seek_times_absolute_and_relative() {
        assert_eq!(parse_seek("1:23:45"), Some(Seek::To(5025)));
        assert_eq!(parse_seek("83:10"), Some(Seek::To(4990)));
        assert_eq!(parse_seek("95"), Some(Seek::To(95)));
        assert_eq!(parse_seek(" 0 "), Some(Seek::To(0)));
        assert_eq!(parse_seek("1:05"), Some(Seek::To(65)));
        assert_eq!(parse_seek("+30"), Some(Seek::By(30)));
        assert_eq!(parse_seek("-10"), Some(Seek::By(-10)));
        assert_eq!(parse_seek("+1:30"), Some(Seek::By(90)));
        assert_eq!(parse_seek("−1:00"), Some(Seek::By(-60)));
        for bad in [
            "", "+", "-", "abc", "1:", ":30", "1::2", "1:60", "1:2:3:4", "1.5", "+-3", "1:234",
            "1234567", "30s", "1 30",
        ] {
            assert_eq!(parse_seek(bad), None, "{}", bad);
        }
    }

    #[test]
    fn clocks_and_playback_answers() {
        assert_eq!(clock(5025.7), "1:23:45");
        assert_eq!(clock(65.0), "1:05");
        assert_eq!(clock(-3.0), "0:00");
        assert_eq!(clock(f64::NAN), "0:00");
        let done = |v: Value| playback_done(&v);
        assert_eq!(done(serde_json::json!({ "action": "play" })), "Playing.");
        assert_eq!(done(serde_json::json!({ "action": "pause" })), "Paused.");
        assert_eq!(
            done(serde_json::json!({ "action": "seek", "position": 5025.2 })),
            "Jumped to 1:23:45."
        );
        assert_eq!(done(Value::Null), "Done.");
        assert_eq!(
            Playback::from(parse_seek("-10").unwrap()),
            Playback::SeekBy(-10)
        );
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
