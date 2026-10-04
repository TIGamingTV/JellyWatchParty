use std::time::{SystemTime, UNIX_EPOCH};

/// Returns the current time in milliseconds since UNIX epoch.
/// Uses saturating arithmetic to handle clock drift gracefully (fixes L01).
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default() // Returns Duration::ZERO if clock went backwards
        .as_millis() as u64
}

/// 64 hex chars (244 random bits) from the OS CSPRNG, via two UUIDv4s.
/// Used for resume secrets and admin session tokens.
pub fn random_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_now_ms_returns_reasonable_value() {
        let ts = now_ms();
        // Should be after 2020-01-01 (1577836800000 ms)
        assert!(ts > 1577836800000, "Timestamp should be after 2020");
        // Should be before 2100-01-01 (4102444800000 ms)
        assert!(ts < 4102444800000, "Timestamp should be before 2100");
    }

    #[test]
    fn random_token_is_long_and_unique() {
        let a = random_token();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, random_token());
    }

    #[test]
    fn test_now_ms_monotonic() {
        let ts1 = now_ms();
        let ts2 = now_ms();
        assert!(ts2 >= ts1, "Timestamps should be monotonically increasing");
    }
}
