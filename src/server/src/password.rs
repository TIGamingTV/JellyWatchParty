use sha2::{Digest, Sha256};

/// Hashes a room password with a fresh random salt.
///
/// Uses a single fast SHA-256 pass rather than a deliberately slow KDF
/// (argon2/bcrypt): rooms are in-memory and gone on restart, so there's no
/// persisted hash database to protect against offline cracking, and anyone
/// able to read process memory can read plaintext passwords off incoming
/// frames anyway. A slow KDF would also run under the global room/client
/// locks on every join attempt, turning wrong-password spam into a
/// server-wide stall. The threat model is "keep a random logged-in Jellyfin
/// user from wandering into a private room," not resisting a dedicated
/// cracking rig; online guessing is bounded by the failed-join throttle in
/// `ws::handlers::join`.
pub fn hash_password(password: &str) -> (String, String) {
    let salt = uuid::Uuid::new_v4().to_string();
    let hash = hash_with_salt(password, &salt);
    (salt, hash)
}

/// Checks a candidate password against a stored (salt, hash) pair in
/// constant time with respect to the hash contents.
pub fn verify_password(candidate: &str, salt: &str, expected_hash: &str) -> bool {
    ct_eq(
        hash_with_salt(candidate, salt).as_bytes(),
        expected_hash.as_bytes(),
    )
}

fn hash_with_salt(password: &str, salt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt.as_bytes());
    hasher.update(password.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Constant-time byte comparison. Length isn't secret here (always 64 hex
/// chars), so an early return on mismatched length is fine.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_roundtrip() {
        let (salt, hash) = hash_password("correct horse battery staple");
        assert!(verify_password(
            "correct horse battery staple",
            &salt,
            &hash
        ));
    }

    #[test]
    fn verify_rejects_wrong_password() {
        let (salt, hash) = hash_password("correct horse battery staple");
        assert!(!verify_password("wrong password", &salt, &hash));
    }

    #[test]
    fn verify_rejects_malformed_hash() {
        let (salt, _) = hash_password("pw");
        assert!(!verify_password("pw", &salt, ""));
        assert!(!verify_password("pw", &salt, "deadbeef"));
    }

    #[test]
    fn different_salts_produce_different_hashes() {
        let (_, hash1) = hash_password("same password");
        let (_, hash2) = hash_password("same password");
        assert_ne!(
            hash1, hash2,
            "salts should differ, producing different hashes"
        );
    }

    #[test]
    fn ct_eq_behaves_like_eq() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(ct_eq(b"", b""));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
        assert!(!ct_eq(b"ab", b"abc"));
    }
}
