//! Admin login: one shared account, in-memory sessions, brute-force throttle.

use super::config::AdminConfig;
use crate::password::ct_eq;
use crate::utils::random_token;
use axum::http::{header, HeaderMap};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::IpAddr;

pub const COOKIE_NAME: &str = "jwp_admin";
/// Failed logins allowed per client IP per window.
pub const LOGIN_FAILS_PER_IP: u32 = 5;
/// Failed logins allowed from everyone together per window. Caps a
/// distributed guesser at the cost of possibly locking the real admin out
/// for a minute while under attack.
pub const LOGIN_FAILS_GLOBAL: u32 = 30;
pub const LOGIN_WINDOW_MS: u64 = 60_000;
/// Oldest sessions are dropped beyond this many.
const MAX_SESSIONS: usize = 100;

#[derive(Default)]
pub struct AuthStore {
    /// token -> expiry (ms since epoch)
    sessions: HashMap<String, u64>,
    /// ip -> (failures, window start)
    per_ip: HashMap<IpAddr, (u32, u64)>,
    global: (u32, u64),
}

fn window_remaining(entry: (u32, u64), limit: u32, now: u64) -> Option<u64> {
    let elapsed = now.saturating_sub(entry.1);
    (entry.0 >= limit && elapsed < LOGIN_WINDOW_MS).then(|| LOGIN_WINDOW_MS - elapsed)
}

fn bump(entry: &mut (u32, u64), now: u64) {
    if now.saturating_sub(entry.1) >= LOGIN_WINDOW_MS {
        *entry = (0, now);
    }
    entry.0 += 1;
}

impl AuthStore {
    /// If logins from `ip` are currently refused, how many ms until retry.
    pub fn login_blocked(&self, ip: IpAddr, now: u64) -> Option<u64> {
        let ip_wait = self
            .per_ip
            .get(&ip)
            .and_then(|e| window_remaining(*e, LOGIN_FAILS_PER_IP, now));
        let global_wait = window_remaining(self.global, LOGIN_FAILS_GLOBAL, now);
        ip_wait.max(global_wait)
    }

    pub fn record_failure(&mut self, ip: IpAddr, now: u64) {
        self.per_ip
            .retain(|_, (_, start)| now.saturating_sub(*start) < LOGIN_WINDOW_MS);
        bump(self.per_ip.entry(ip).or_insert((0, now)), now);
        bump(&mut self.global, now);
    }

    pub fn clear_failures(&mut self, ip: IpAddr) {
        self.per_ip.remove(&ip);
    }

    pub fn create_session(&mut self, now: u64, ttl_ms: u64) -> String {
        self.sessions.retain(|_, exp| *exp > now);
        while self.sessions.len() >= MAX_SESSIONS {
            let oldest = self
                .sessions
                .iter()
                .min_by_key(|(_, exp)| **exp)
                .map(|(t, _)| t.clone());
            match oldest {
                Some(t) => self.sessions.remove(&t),
                None => break,
            };
        }
        let token = random_token();
        self.sessions.insert(token.clone(), now + ttl_ms);
        token
    }

    pub fn is_valid(&self, token: &str, now: u64) -> bool {
        self.sessions.get(token).is_some_and(|exp| *exp > now)
    }

    pub fn revoke(&mut self, token: &str) {
        self.sessions.remove(token);
    }
}

fn digest(s: &str) -> [u8; 32] {
    Sha256::digest(s.as_bytes()).into()
}

/// Compares fixed-length digests in constant time, so neither the content
/// nor the length of the configured credentials leaks through timing.
pub fn credentials_match(cfg: &AdminConfig, username: &str, password: &str) -> bool {
    let user_ok = ct_eq(&digest(username), &digest(&cfg.username));
    let pass_ok = ct_eq(&digest(password), &digest(&cfg.password));
    user_ok & pass_ok
}

pub fn cookie_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE_NAME)
        .map(|(_, v)| v.to_string())
        .filter(|v| !v.is_empty())
}

pub fn session_cookie(token: &str, ttl_ms: u64, secure: bool) -> String {
    format!(
        "{}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{}",
        COOKIE_NAME,
        token,
        ttl_ms / 1000,
        if secure { "; Secure" } else { "" }
    )
}

pub fn clear_cookie(secure: bool) -> String {
    session_cookie("", 0, secure)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn cfg() -> AdminConfig {
        AdminConfig {
            addr: "127.0.0.1:0".parse().unwrap(),
            username: "admin".into(),
            password: "correct horse".into(),
            session_ttl_ms: 1000,
            cookie_secure: false,
            empty_group_ttl_ms: 1000,
            trust_forwarded_for: false,
        }
    }

    #[test]
    fn credentials() {
        let c = cfg();
        assert!(credentials_match(&c, "admin", "correct horse"));
        assert!(!credentials_match(&c, "admin", "correct hors"));
        assert!(!credentials_match(&c, "Admin", "correct horse"));
        assert!(!credentials_match(&c, "", ""));
    }

    #[test]
    fn sessions_expire_and_revoke() {
        let mut s = AuthStore::default();
        let t = s.create_session(1_000, 500);
        assert!(s.is_valid(&t, 1_400));
        assert!(!s.is_valid(&t, 1_500));
        assert!(!s.is_valid("nope", 1_000));
        let t2 = s.create_session(2_000, 500);
        s.revoke(&t2);
        assert!(!s.is_valid(&t2, 2_001));
    }

    #[test]
    fn session_count_is_bounded() {
        let mut s = AuthStore::default();
        let first = s.create_session(0, 10_000);
        for i in 1..=MAX_SESSIONS as u64 {
            s.create_session(i, 10_000);
        }
        assert!(s.sessions.len() <= MAX_SESSIONS);
        assert!(!s.is_valid(&first, 5));
    }

    #[test]
    fn per_ip_throttle() {
        let mut s = AuthStore::default();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let other: IpAddr = "10.0.0.2".parse().unwrap();
        for _ in 0..LOGIN_FAILS_PER_IP {
            assert_eq!(s.login_blocked(ip, 1_000), None);
            s.record_failure(ip, 1_000);
        }
        assert_eq!(s.login_blocked(ip, 11_000), Some(LOGIN_WINDOW_MS - 10_000));
        assert_eq!(s.login_blocked(other, 11_000), None);
        assert_eq!(s.login_blocked(ip, 1_000 + LOGIN_WINDOW_MS), None);
        s.clear_failures(ip);
        assert_eq!(s.login_blocked(ip, 2_000), None);
    }

    #[test]
    fn global_throttle() {
        let mut s = AuthStore::default();
        for i in 0..LOGIN_FAILS_GLOBAL {
            s.record_failure(
                IpAddr::from([10, 0, (i / 250) as u8, (i % 250) as u8]),
                1_000,
            );
        }
        assert!(s
            .login_blocked("192.168.1.1".parse().unwrap(), 1_000)
            .is_some());
    }

    #[test]
    fn cookie_parsing() {
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            HeaderValue::from_static("a=b; jwp_admin=tok123; c=d"),
        );
        assert_eq!(cookie_token(&h).as_deref(), Some("tok123"));
        let mut empty = HeaderMap::new();
        empty.insert(header::COOKIE, HeaderValue::from_static("jwp_admin="));
        assert_eq!(cookie_token(&empty), None);
        assert_eq!(cookie_token(&HeaderMap::new()), None);
    }

    #[test]
    fn cookie_attributes() {
        let c = session_cookie("t", 60_000, true);
        assert!(c.contains("HttpOnly"));
        assert!(c.contains("SameSite=Strict"));
        assert!(c.contains("Max-Age=60"));
        assert!(c.ends_with("; Secure"));
        assert!(!session_cookie("t", 1000, false).contains("Secure"));
        assert!(clear_cookie(false).contains("Max-Age=0"));
    }
}
