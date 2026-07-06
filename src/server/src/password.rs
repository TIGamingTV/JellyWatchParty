use sha2::{Digest, Sha256};

/// Hashes a room password with a fresh random salt.
///
/// Uses a single fast SHA-256 pass rather than a deliberately slow KDF
/// (argon2/bcrypt): rooms are in-memory and gone on restart, so there's no
/// persisted hash database to protect against offline cracking. The threat
/// model is "keep a random logged-in Jellyfin user from wandering into a
/// private room," not resisting a dedicated cracking rig.
pub fn hash_password(password: &str) -> (String, String) {
    let salt = uuid::Uuid::new_v4().to_string();
    let hash = hash_with_salt(password, &salt);
    (salt, hash)
}

/// Checks a candidate password against a stored (salt, hash) pair.
pub fn verify_password(candidate: &str, salt: &str, expected_hash: &str) -> bool {
    hash_with_salt(candidate, salt) == expected_hash
}

fn hash_with_salt(password: &str, salt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt.as_bytes());
    hasher.update(password.as_bytes());
    format!("{:x}", hasher.finalize())
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
    fn different_salts_produce_different_hashes() {
        let (_, hash1) = hash_password("same password");
        let (_, hash2) = hash_password("same password");
        assert_ne!(
            hash1, hash2,
            "salts should differ, producing different hashes"
        );
    }
}
