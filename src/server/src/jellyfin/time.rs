//! Parses the ISO-8601 UTC timestamps Jellyfin puts in session JSON (e.g.
//! `2026-10-03T19:52:36.1234567Z`) without pulling in a date crate.

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `YYYY-MM-DDTHH:MM:SS[.fraction](Z|+00:00)` to ms since the Unix epoch.
/// Returns `None` for anything else, including non-UTC offsets (Jellyfin
/// always sends UTC).
pub fn parse_utc_ms(s: &str) -> Option<u64> {
    let s = s.trim();
    let body = s.strip_suffix('Z').or_else(|| s.strip_suffix("+00:00"))?;
    let (date, time) = body.split_once('T')?;
    let mut dp = date.splitn(3, '-');
    let year: i64 = dp.next()?.parse().ok()?;
    let month: i64 = dp.next()?.parse().ok()?;
    let day: i64 = dp.next()?.parse().ok()?;
    let (hms, frac) = match time.split_once('.') {
        Some((a, b)) => (a, b),
        None => (time, ""),
    };
    let mut tp = hms.splitn(3, ':');
    let hour: i64 = tp.next()?.parse().ok()?;
    let min: i64 = tp.next()?.parse().ok()?;
    let sec: i64 = tp.next()?.parse().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || min > 59
        || sec > 60
        || !frac.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let millis: i64 = frac
        .chars()
        .chain(std::iter::repeat('0'))
        .take(3)
        .collect::<String>()
        .parse()
        .ok()?;
    let secs = days_from_civil(year, month, day) * 86_400 + hour * 3600 + min * 60 + sec;
    u64::try_from(secs * 1000 + millis).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_jellyfin_timestamps() {
        assert_eq!(parse_utc_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_utc_ms("2026-10-03T19:52:36.1234567Z"),
            Some(1_791_057_156_123)
        );
        assert_eq!(
            parse_utc_ms("2000-02-29T12:00:00.5Z"),
            Some(951_825_600_500)
        );
        assert_eq!(
            parse_utc_ms("2026-10-03T19:52:36+00:00"),
            Some(1_791_057_156_000)
        );
    }

    #[test]
    fn rejects_garbage() {
        for s in [
            "",
            "0001-01-01",
            "2026-13-01T00:00:00Z",
            "2026-10-03T19:52:36",
            "2026-10-03T19:52:36+02:00",
            "2026-10-03T25:00:00Z",
            "2026-10-03T19:52:36.12a4Z",
            "1969-12-31T23:59:59Z",
        ] {
            assert_eq!(parse_utc_ms(s), None, "{}", s);
        }
    }
}
