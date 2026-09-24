use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

/// Hashes a room password with a fresh random salt.
///
/// Uses Argon2id, a deliberately memory-hard KDF, so that even if room
/// state (and its hashes) is extracted from memory, offline cracking is
/// far more expensive than with a fast general-purpose digest like SHA-256.
pub fn hash_password(password: &str) -> (String, String) {
    let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    let hash = hash_with_salt(password, &salt);
    (salt.to_string(), hash)
}

/// Checks a candidate password against a stored (salt, hash) pair.
pub fn verify_password(candidate: &str, salt: &str, expected_hash: &str) -> bool {
    let Ok(parsed_hash) = PasswordHash::new(expected_hash) else {
        return false;
    };
    let _ = salt;
    Argon2::default()
        .verify_password(candidate.as_bytes(), &parsed_hash)
        .is_ok()
}

fn hash_with_salt(password: &str, salt: &SaltString) -> String {
    Argon2::default()
        .hash_password(password.as_bytes(), salt)
        .expect("argon2 hashing failed")
        .to_string()
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
