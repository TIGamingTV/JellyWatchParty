use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub name: String,
    pub aud: String,
    pub iss: String,
    pub exp: usize,
    pub iat: usize,
}

const MIN_ENTROPY_BITS: f64 = 80.0;

fn calculate_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let mut freq: HashMap<char, usize> = HashMap::new();
    for c in s.chars() {
        *freq.entry(c).or_insert(0) += 1;
    }
    let len = s.len() as f64;
    let entropy: f64 = freq
        .values()
        .map(|&count| {
            let p = count as f64 / len;
            -p * p.log2()
        })
        .sum();
    entropy * len
}

fn log_disabled_warning() {
    log::warn!("=======================================================");
    log::warn!("SECURITY WARNING: JWT_SECRET not set!");
    log::warn!("Authentication is DISABLED - anyone can join rooms.");
    log::warn!("This is acceptable for development/private networks.");
    log::warn!("For production, set JWT_SECRET environment variable.");
    log::warn!("=======================================================");
}

fn validate_secret_quality(secret: &str) {
    if secret.len() < 32 {
        log::warn!(
            "JWT_SECRET is too short. Use at least 32 characters for secure authentication."
        );
    }
    let entropy = calculate_entropy(secret);
    if entropy < MIN_ENTROPY_BITS {
        log::warn!(
            "JWT_SECRET has low entropy ({:.1} bits, minimum recommended: {:.0} bits). \
             Use a cryptographically random secret for secure authentication.",
            entropy,
            MIN_ENTROPY_BITS
        );
    }
}

#[derive(Clone)]
pub struct JwtConfig {
    pub secret: String,
    pub audience: String,
    pub issuer: String,
    pub enabled: bool,
}

impl JwtConfig {
    pub fn from_env() -> Self {
        let secret = std::env::var("JWT_SECRET").unwrap_or_default();
        let enabled = !secret.is_empty();

        if !enabled {
            log_disabled_warning();
        } else {
            validate_secret_quality(&secret);
        }

        Self {
            secret,
            audience: std::env::var("JWT_AUDIENCE")
                .unwrap_or_else(|_| "JellyWatchParty".to_string()),
            issuer: std::env::var("JWT_ISSUER").unwrap_or_else(|_| "Jellyfin".to_string()),
            enabled,
        }
    }

    pub fn validate_token(&self, token: &str) -> Result<Claims, String> {
        if !self.enabled {
            return Ok(Claims {
                sub: "anonymous".to_string(),
                name: "Anonymous".to_string(),
                aud: self.audience.clone(),
                iss: self.issuer.clone(),
                exp: 0,
                iat: 0,
            });
        }

        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_audience(&[&self.audience]);
        validation.set_issuer(&[&self.issuer]);
        validation.validate_exp = true;
        validation.leeway = 60;

        match decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &validation,
        ) {
            Ok(token_data) => Ok(token_data.claims),
            Err(e) => Err(format!("Invalid token: {}", e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entropy_empty_string() {
        assert_eq!(calculate_entropy(""), 0.0);
    }

    #[test]
    fn test_entropy_single_char() {
        let entropy = calculate_entropy("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert!(
            entropy < 1.0,
            "Repeated single char should have near-zero entropy"
        );
    }

    #[test]
    fn test_entropy_two_chars() {
        let entropy = calculate_entropy("abababababababababababababababab");
        assert!(
            entropy > 10.0 && entropy < 40.0,
            "Two char alternating should have low entropy: {}",
            entropy
        );
    }

    #[test]
    fn test_entropy_random_looking() {
        let entropy = calculate_entropy("aB3$xY9!pQ2@wE5#rT8^uI1&oP4*");
        assert!(
            entropy > MIN_ENTROPY_BITS,
            "Random-looking string should have high entropy: {}",
            entropy
        );
    }

    #[test]
    fn test_entropy_uuid() {
        let entropy = calculate_entropy("550e8400e29b41d4a716446655440000");
        assert!(
            entropy > 60.0,
            "UUID should have reasonable entropy: {}",
            entropy
        );
    }

    #[test]
    fn test_entropy_weak_password() {
        let entropy = calculate_entropy("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaabb");
        assert!(
            entropy < MIN_ENTROPY_BITS,
            "Weak pattern should have low entropy: {}",
            entropy
        );
    }

    #[test]
    fn test_jwt_config_disabled() {
        std::env::remove_var("JWT_SECRET");
        let config = JwtConfig::from_env();
        assert!(
            !config.enabled,
            "Auth should be disabled when JWT_SECRET is empty"
        );
    }

    #[test]
    fn test_jwt_validate_when_disabled() {
        let config = JwtConfig {
            secret: String::new(),
            audience: "test".to_string(),
            issuer: "test".to_string(),
            enabled: false,
        };
        let result = config.validate_token("any-token");
        assert!(result.is_ok(), "Should succeed when auth disabled");
        let claims = result.unwrap();
        assert_eq!(claims.name, "Anonymous");
    }

    #[test]
    fn test_jwt_validate_invalid_token() {
        let config = JwtConfig {
            secret: "test-secret-with-at-least-32-characters-here".to_string(),
            audience: "test".to_string(),
            issuer: "test".to_string(),
            enabled: true,
        };
        let result = config.validate_token("invalid-token");
        assert!(result.is_err(), "Should fail for invalid token");
    }
}
