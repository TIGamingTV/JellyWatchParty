//! The chat integration's data file, `DATA_DIR/integrations.json`: settings
//! the admin edits in the UI, link codes and the chat accounts linked to
//! Jellyfin users. Written atomically (temp file + rename, mode 0600).
//!
//! Codes are never stored: only HMAC-SHA256(install key, user id, code),
//! with the key in `DATA_DIR/secret.key`. With 10,000 possible codes that
//! doesn't stop someone holding both files from finding a code; it keeps
//! codes out of casual view (logs, backups of the JSON alone). What really
//! protects a code are the attempt limits: see `CODE_FAILS_BEFORE_FREEZE`
//! and the per-account lockout in `integration::Integration::link`.

use crate::password::ct_eq;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

const DATA_FILE: &str = "integrations.json";
const KEY_FILE: &str = "secret.key";
const FORMAT_VERSION: u32 = 1;
/// Wrong codes entered for one Jellyfin user (by anyone) before the code
/// stops working until an admin assigns a new one. Bounds the chance of
/// guessing a code at 10 / 10,000 per assignment.
pub const CODE_FAILS_BEFORE_FREEZE: u32 = 10;
/// Max length of a Discord snowflake (Telegram ids are shorter).
const MAX_ID_LEN: usize = 20;
const MAX_CHANNELS: usize = 25;
/// Telegram has no roles: its bot reports group administrators with this
/// one, and `admin_role_id` set to it makes them admins in every room.
pub const GROUP_ADMIN_ROLE: &str = "admin";

/// Per-platform settings, edited in the admin UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChatSettings {
    pub enabled: bool,
    /// The one place the bot works in: a Discord server (guild), or a
    /// Telegram group (a negative chat id).
    pub guild_id: String,
    /// Channels the commands are accepted in (empty: any channel).
    /// Discord only.
    pub channel_ids: Vec<String>,
    /// Only members with this role may use the bot (empty: everyone).
    /// Discord only.
    pub required_role_id: String,
    /// Members with this role count as admins in every chat room (empty:
    /// only Jellyfin administrators do). On Telegram either empty or
    /// `GROUP_ADMIN_ROLE`.
    pub admin_role_id: String,
    pub max_rooms_per_user: u32,
    pub max_rooms_total: u32,
    pub require_password: bool,
    /// Whether users may add their devices as host / as receiver.
    pub allow_host: bool,
    pub allow_receiver: bool,
    /// A room without members closes after this long.
    pub empty_room_minutes: u32,
}

impl Default for ChatSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            guild_id: String::new(),
            channel_ids: Vec::new(),
            required_role_id: String::new(),
            admin_role_id: String::new(),
            max_rooms_per_user: 2,
            max_rooms_total: 20,
            require_password: false,
            allow_host: true,
            allow_receiver: true,
            empty_room_minutes: 30,
        }
    }
}

fn is_snowflake(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_ID_LEN && s.bytes().all(|b| b.is_ascii_digit())
}

/// A Telegram group or supergroup id: a negative number.
fn is_group_id(s: &str) -> bool {
    s.strip_prefix('-')
        .is_some_and(|n| is_snowflake(n) && n.bytes().any(|b| b != b'0'))
}

impl ChatSettings {
    /// Trims and checks admin input for `provider`'s settings.
    pub fn validate(mut self, provider: &str) -> Result<Self, String> {
        self.guild_id = self.guild_id.trim().to_string();
        self.required_role_id = self.required_role_id.trim().to_string();
        self.admin_role_id = self.admin_role_id.trim().to_string();
        let mut channels: Vec<String> = Vec::new();
        for c in &self.channel_ids {
            let c = c.trim().to_string();
            if !c.is_empty() && !channels.contains(&c) {
                channels.push(c);
            }
        }
        self.channel_ids = channels;

        match provider {
            "telegram" => self.check_telegram()?,
            _ => self.check_discord()?,
        }
        if !(1..=10).contains(&self.max_rooms_per_user) {
            return Err("Rooms per user must be between 1 and 10".into());
        }
        if !(1..=100).contains(&self.max_rooms_total) {
            return Err("Rooms in total must be between 1 and 100".into());
        }
        if !(5..=1440).contains(&self.empty_room_minutes) {
            return Err("Empty rooms must close after 5 to 1440 minutes".into());
        }
        Ok(self)
    }

    fn check_discord(&self) -> Result<(), String> {
        if !self.guild_id.is_empty() && !is_snowflake(&self.guild_id) {
            return Err(
                "Server ID must be a number (Discord: right-click the server > Copy Server ID)"
                    .into(),
            );
        }
        if self.channel_ids.len() > MAX_CHANNELS {
            return Err(format!("At most {} channels", MAX_CHANNELS));
        }
        if let Some(bad) = self.channel_ids.iter().find(|c| !is_snowflake(c)) {
            return Err(format!("Channel ID '{}' is not a number", bad));
        }
        for (label, id) in [
            ("Required role", &self.required_role_id),
            ("Admin role", &self.admin_role_id),
        ] {
            if !id.is_empty() && !is_snowflake(id) {
                return Err(format!("{} ID must be a number", label));
            }
        }
        if self.enabled && self.guild_id.is_empty() {
            return Err("Set the server ID before enabling the bot".into());
        }
        Ok(())
    }

    fn check_telegram(&self) -> Result<(), String> {
        if !self.guild_id.is_empty() && !is_group_id(&self.guild_id) {
            return Err(
                "Group ID must be a negative number (send /groupid in the group to see it)".into(),
            );
        }
        if !self.channel_ids.is_empty() {
            return Err("Telegram has no channel list: leave it empty".into());
        }
        if !self.required_role_id.is_empty() {
            return Err("Telegram has no roles: leave the required role empty".into());
        }
        if !self.admin_role_id.is_empty() && self.admin_role_id != GROUP_ADMIN_ROLE {
            return Err(format!(
                "On Telegram the admin role is either empty or '{}' (group administrators)",
                GROUP_ADMIN_ROLE
            ));
        }
        if self.enabled && self.guild_id.is_empty() {
            return Err("Set the group ID before enabling the bot".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub discord: ChatSettings,
    /// Absent in data files written before Telegram support.
    #[serde(default)]
    pub telegram: ChatSettings,
}

impl Settings {
    pub fn for_provider(&self, provider: &str) -> Option<&ChatSettings> {
        match provider {
            "discord" => Some(&self.discord),
            "telegram" => Some(&self.telegram),
            _ => None,
        }
    }

    pub fn for_provider_mut(&mut self, provider: &str) -> Option<&mut ChatSettings> {
        match provider {
            "discord" => Some(&mut self.discord),
            "telegram" => Some(&mut self.telegram),
            _ => None,
        }
    }
}

/// A chat account linked to a Jellyfin user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub external_id: String,
    pub display_name: String,
    pub linked_at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserRecord {
    /// Jellyfin user name when last seen (for display).
    pub name: String,
    #[serde(default)]
    pub code_hmac: Option<String>,
    #[serde(default)]
    pub assigned_at: u64,
    /// Wrong codes entered for this user since the code was assigned.
    #[serde(default)]
    pub failed: u32,
    #[serde(default)]
    pub frozen: bool,
    /// Provider -> linked account.
    #[serde(default)]
    pub links: BTreeMap<String, Link>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreData {
    pub version: u32,
    #[serde(default)]
    pub settings: Settings,
    /// Normalized Jellyfin user id -> record.
    #[serde(default)]
    pub users: BTreeMap<String, UserRecord>,
}

impl Default for StoreData {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            settings: Settings::default(),
            users: BTreeMap::new(),
        }
    }
}

/// Result of checking a code for a user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeCheck {
    Match,
    Wrong {
        frozen_now: bool,
    },
    /// No code assigned (or revoked).
    NoCode,
    /// Too many wrong codes; waits for an admin.
    Frozen,
}

impl StoreData {
    /// Sets a new code (as its HMAC) for `user_id`, dropping existing links
    /// and the failure count.
    pub fn assign_code(&mut self, user_id: &str, name: &str, code_hmac: String, now: u64) {
        let rec = self.users.entry(user_id.to_string()).or_default();
        rec.name = name.to_string();
        rec.code_hmac = Some(code_hmac);
        rec.assigned_at = now;
        rec.failed = 0;
        rec.frozen = false;
        rec.links.clear();
    }

    /// Removes the code and every link. Returns false if there was nothing.
    pub fn revoke_code(&mut self, user_id: &str) -> bool {
        self.users.remove(user_id).is_some()
    }

    /// Unlinks `provider` from `user_id`. Returns false if it wasn't linked.
    pub fn unlink(&mut self, user_id: &str, provider: &str) -> bool {
        self.users
            .get_mut(user_id)
            .is_some_and(|r| r.links.remove(provider).is_some())
    }

    /// The Jellyfin user a chat account is linked to.
    pub fn linked_user(&self, provider: &str, external_id: &str) -> Option<(&str, &UserRecord)> {
        self.users
            .iter()
            .find(|(_, r)| {
                r.links
                    .get(provider)
                    .is_some_and(|l| l.external_id == external_id)
            })
            .map(|(id, r)| (id.as_str(), r))
    }

    /// The chat account linked to a Jellyfin user.
    pub fn link_of(&self, user_id: &str, provider: &str) -> Option<&Link> {
        self.users.get(user_id)?.links.get(provider)
    }

    /// Checks `code_hmac` against the user's code, counting a wrong one.
    pub fn check_code(&mut self, user_id: &str, code_hmac: &str) -> CodeCheck {
        let Some(rec) = self.users.get_mut(user_id) else {
            return CodeCheck::NoCode;
        };
        let Some(expected) = rec.code_hmac.as_deref() else {
            return CodeCheck::NoCode;
        };
        if rec.frozen {
            return CodeCheck::Frozen;
        }
        if ct_eq(expected.as_bytes(), code_hmac.as_bytes()) {
            rec.failed = 0;
            return CodeCheck::Match;
        }
        rec.failed = rec.failed.saturating_add(1);
        let frozen_now = rec.failed >= CODE_FAILS_BEFORE_FREEZE;
        rec.frozen = frozen_now;
        CodeCheck::Wrong { frozen_now }
    }

    /// Links `link.external_id` on `provider` to `user_id`. One chat account
    /// belongs to one Jellyfin user: a link it had to another user goes.
    pub fn set_link(&mut self, user_id: &str, name: &str, provider: &str, link: Link) {
        for (id, r) in self.users.iter_mut() {
            if id != user_id
                && r.links
                    .get(provider)
                    .is_some_and(|l| l.external_id == link.external_id)
            {
                r.links.remove(provider);
            }
        }
        let rec = self.users.entry(user_id.to_string()).or_default();
        rec.name = name.to_string();
        rec.links.insert(provider.to_string(), link);
    }
}

/// A fresh 4-digit code from the OS CSPRNG (uniform: rejection sampling).
pub fn generate_code() -> String {
    loop {
        let mut b = [0u8; 2];
        getrandom::fill(&mut b).expect("the OS random number generator failed");
        let n = u16::from_le_bytes(b);
        if n < 60_000 {
            return format!("{:04}", n % 10_000);
        }
    }
}

/// A code as typed: 4 digits, spaces and dashes ignored.
pub fn clean_code(raw: &str) -> Option<String> {
    let code: String = raw.chars().filter(|c| !matches!(c, ' ' | '-')).collect();
    (code.len() == 4 && code.bytes().all(|b| b.is_ascii_digit())).then_some(code)
}

/// Outcome of a change: the closure's value, and whether it reached disk.
/// The in-memory state is updated either way, so attempt counters keep
/// counting even if the disk is full.
pub struct Updated<R> {
    pub value: R,
    pub saved: Result<(), String>,
}

pub struct Store {
    path: PathBuf,
    key: [u8; 32],
    data: Mutex<StoreData>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Creates `path`, which must not exist yet, readable by the owner only.
fn create_private(path: &Path) -> std::io::Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
}

fn load_or_create_key(dir: &Path) -> Result<[u8; 32], String> {
    let path = dir.join(KEY_FILE);
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let bytes = unhex(s.trim())
                .filter(|b| b.len() == 32)
                .ok_or_else(|| format!("{} is not a valid key file", path.display()))?;
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            Ok(key)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut key = [0u8; 32];
            getrandom::fill(&mut key).map_err(|e| format!("random key: {}", e))?;
            let mut f = create_private(&path)
                .map_err(|e| format!("cannot create {}: {}", path.display(), e))?;
            f.write_all(hex(&key).as_bytes())
                .and_then(|_| f.sync_all())
                .map_err(|e| format!("cannot write {}: {}", path.display(), e))?;
            log::info!("Created a new integration key in {}", path.display());
            Ok(key)
        }
        Err(e) => Err(format!("cannot read {}: {}", path.display(), e)),
    }
}

impl Store {
    /// Opens (or starts) the data file in `dir`. A data file that can't be
    /// parsed is an error, never silently replaced.
    pub fn open(dir: &Path) -> Result<Self, String> {
        let existed = dir.exists();
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create DATA_DIR {}: {}", dir.display(), e))?;
        #[cfg(unix)]
        if !existed {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
        let key = load_or_create_key(dir)?;
        let path = dir.join(DATA_FILE);
        let data = match std::fs::read_to_string(&path) {
            Ok(s) => {
                let data: StoreData = serde_json::from_str(&s).map_err(|e| {
                    format!(
                        "{} is not valid ({}); fix it or move it away",
                        path.display(),
                        e
                    )
                })?;
                if data.version > FORMAT_VERSION {
                    return Err(format!(
                        "{} was written by a newer version of the server",
                        path.display()
                    ));
                }
                data
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => StoreData::default(),
            Err(e) => return Err(format!("cannot read {}: {}", path.display(), e)),
        };
        let store = Self {
            path,
            key,
            data: Mutex::new(data),
        };
        // Fail now, not at the first link, if the directory isn't writable.
        let snapshot = store.read(|d| d.clone());
        store.write(&snapshot)?;
        Ok(store)
    }

    fn lock(&self) -> MutexGuard<'_, StoreData> {
        self.data.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn read<R>(&self, f: impl FnOnce(&StoreData) -> R) -> R {
        f(&self.lock())
    }

    /// Changes the data and writes it to disk (while holding the lock, so
    /// writes never go out of order).
    pub fn update<R>(&self, f: impl FnOnce(&mut StoreData) -> R) -> Updated<R> {
        let mut data = self.lock();
        let before = data.clone();
        let value = f(&mut data);
        let saved = if *data == before {
            Ok(())
        } else {
            self.write(&data)
        };
        if let Err(e) = &saved {
            log::error!("Chat integration data not saved: {}", e);
        }
        Updated { value, saved }
    }

    fn write(&self, data: &StoreData) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(data).map_err(|e| e.to_string())?;
        let tmp = self.path.with_extension("json.tmp");
        // A leftover temp file (crash, older version) would keep its own
        // permissions when reopened: start from a fresh, private one.
        let _ = std::fs::remove_file(&tmp);
        let mut f =
            create_private(&tmp).map_err(|e| format!("cannot write {}: {}", tmp.display(), e))?;
        f.write_all(&json)
            .and_then(|_| f.sync_all())
            .map_err(|e| format!("cannot write {}: {}", tmp.display(), e))?;
        drop(f);
        std::fs::rename(&tmp, &self.path)
            .map_err(|e| format!("cannot replace {}: {}", self.path.display(), e))?;
        if let Some(dir) = self.path.parent() {
            // Make the rename itself durable; not fatal where unsupported.
            if let Ok(d) = std::fs::File::open(dir) {
                let _ = d.sync_all();
            }
        }
        Ok(())
    }

    /// HMAC of a code for one user (a code only works for its own user).
    pub fn code_mac(&self, user_id: &str, code: &str) -> String {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.key)
            .expect("HMAC takes a key of any length");
        mac.update(b"jwp-link-code\0");
        mac.update(user_id.as_bytes());
        mac.update(b"\0");
        mac.update(code.as_bytes());
        hex(&mac.finalize().into_bytes())
    }
}

#[cfg(test)]
pub(crate) fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("jwp-store-{}", uuid::Uuid::new_v4()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(id: &str) -> Link {
        Link {
            external_id: id.into(),
            display_name: format!("name-{}", id),
            linked_at: 1,
        }
    }

    #[test]
    fn codes_are_four_uniform_digits() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..2000 {
            let c = generate_code();
            assert_eq!(c.len(), 4);
            assert!(c.bytes().all(|b| b.is_ascii_digit()));
            seen.insert(c);
        }
        // 2000 draws from 10,000 values: about 1,800 distinct.
        assert!(seen.len() > 1500, "{}", seen.len());
    }

    #[test]
    fn clean_code_accepts_only_four_digits() {
        assert_eq!(clean_code(" 12 34 ").as_deref(), Some("1234"));
        assert_eq!(clean_code("12-34").as_deref(), Some("1234"));
        assert_eq!(clean_code("123"), None);
        assert_eq!(clean_code("12345"), None);
        assert_eq!(clean_code("12a4"), None);
        assert_eq!(clean_code("١٢٣٤"), None);
    }

    #[test]
    fn a_code_freezes_after_too_many_wrong_guesses() {
        let mut d = StoreData::default();
        d.assign_code("u1", "Alice", "right".into(), 1);
        for i in 1..CODE_FAILS_BEFORE_FREEZE {
            assert_eq!(
                d.check_code("u1", "wrong"),
                CodeCheck::Wrong { frozen_now: false },
                "attempt {}",
                i
            );
        }
        assert_eq!(
            d.check_code("u1", "wrong"),
            CodeCheck::Wrong { frozen_now: true }
        );
        // Even the right code is refused now.
        assert_eq!(d.check_code("u1", "right"), CodeCheck::Frozen);
        // Reassigning unfreezes.
        d.assign_code("u1", "Alice", "new".into(), 2);
        assert_eq!(d.check_code("u1", "new"), CodeCheck::Match);
        assert_eq!(d.check_code("nobody", "x"), CodeCheck::NoCode);
    }

    #[test]
    fn a_right_code_resets_the_failure_count() {
        let mut d = StoreData::default();
        d.assign_code("u1", "Alice", "right".into(), 1);
        for _ in 0..CODE_FAILS_BEFORE_FREEZE - 1 {
            d.check_code("u1", "wrong");
        }
        assert_eq!(d.check_code("u1", "right"), CodeCheck::Match);
        assert_eq!(d.users["u1"].failed, 0);
    }

    #[test]
    fn reassigning_drops_links() {
        let mut d = StoreData::default();
        d.assign_code("u1", "Alice", "c".into(), 1);
        d.set_link("u1", "Alice", "discord", link("100"));
        assert_eq!(d.linked_user("discord", "100").unwrap().0, "u1");
        d.assign_code("u1", "Alice", "c2".into(), 2);
        assert!(d.linked_user("discord", "100").is_none());
    }

    #[test]
    fn one_chat_account_links_to_one_user() {
        let mut d = StoreData::default();
        d.set_link("u1", "Alice", "discord", link("100"));
        d.set_link("u2", "Bob", "discord", link("100"));
        assert_eq!(d.linked_user("discord", "100").unwrap().0, "u2");
        assert!(d.link_of("u1", "discord").is_none());
        assert!(d.unlink("u2", "discord"));
        assert!(!d.unlink("u2", "discord"));
        assert!(d.revoke_code("u1"));
    }

    #[test]
    fn settings_are_validated() {
        let ok = ChatSettings {
            enabled: true,
            guild_id: " 123456789012345678 ".into(),
            channel_ids: vec!["1".into(), " 1 ".into(), "".into(), "2".into()],
            ..Default::default()
        }
        .validate("discord")
        .unwrap();
        assert_eq!(ok.guild_id, "123456789012345678");
        assert_eq!(ok.channel_ids, vec!["1".to_string(), "2".to_string()]);

        let bad = |s: ChatSettings| s.validate("discord").is_err();
        assert!(bad(ChatSettings {
            guild_id: "-100123".into(),
            ..Default::default()
        }));
        assert!(bad(ChatSettings {
            enabled: true,
            ..Default::default()
        }));
        assert!(bad(ChatSettings {
            guild_id: "abc".into(),
            ..Default::default()
        }));
        assert!(bad(ChatSettings {
            channel_ids: vec!["x".into()],
            ..Default::default()
        }));
        assert!(bad(ChatSettings {
            admin_role_id: "1 2".into(),
            ..Default::default()
        }));
        assert!(bad(ChatSettings {
            max_rooms_per_user: 0,
            ..Default::default()
        }));
        assert!(bad(ChatSettings {
            empty_room_minutes: 1,
            ..Default::default()
        }));
        assert!(bad(ChatSettings {
            guild_id: "1".repeat(21),
            ..Default::default()
        }));
    }

    #[test]
    fn telegram_settings_take_a_group_and_no_roles() {
        let ok = ChatSettings {
            enabled: true,
            guild_id: " -1001234567890 ".into(),
            admin_role_id: GROUP_ADMIN_ROLE.into(),
            ..Default::default()
        }
        .validate("telegram")
        .unwrap();
        assert_eq!(ok.guild_id, "-1001234567890");
        assert!(ChatSettings {
            guild_id: "-4567".into(),
            ..Default::default()
        }
        .validate("telegram")
        .is_ok());

        let bad = |s: ChatSettings| s.validate("telegram").is_err();
        assert!(bad(ChatSettings {
            enabled: true,
            ..Default::default()
        }));
        // A user (positive) id, or not a number at all.
        for g in [
            "1234",
            "-",
            "-0",
            "--1",
            "-12a",
            "@group",
            &format!("-{}", "1".repeat(21)),
        ] {
            assert!(
                bad(ChatSettings {
                    guild_id: g.into(),
                    ..Default::default()
                }),
                "{}",
                g
            );
        }
        assert!(bad(ChatSettings {
            channel_ids: vec!["1".into()],
            ..Default::default()
        }));
        assert!(bad(ChatSettings {
            required_role_id: "1".into(),
            ..Default::default()
        }));
        assert!(bad(ChatSettings {
            admin_role_id: "123".into(),
            ..Default::default()
        }));
        assert!(bad(ChatSettings {
            max_rooms_total: 0,
            ..Default::default()
        }));
    }

    #[test]
    fn a_data_file_from_before_telegram_still_loads() {
        let d: StoreData = serde_json::from_str(
            r#"{"version":1,"settings":{"discord":{"enabled":true,"guild_id":"42"}},"users":{}}"#,
        )
        .unwrap();
        assert_eq!(d.settings.discord.guild_id, "42");
        assert_eq!(d.settings.telegram, ChatSettings::default());
        assert!(d.settings.for_provider("telegram").is_some());
        assert!(d.settings.for_provider("matrix").is_none());
    }

    #[test]
    fn one_jellyfin_user_links_one_account_per_platform() {
        let mut d = StoreData::default();
        d.assign_code("u1", "Alice", "c".into(), 1);
        d.set_link("u1", "Alice", "discord", link("100"));
        d.set_link("u1", "Alice", "telegram", link("100"));
        assert_eq!(d.linked_user("discord", "100").unwrap().0, "u1");
        assert_eq!(d.linked_user("telegram", "100").unwrap().0, "u1");
        // Unlinking one platform keeps the other.
        assert!(d.unlink("u1", "telegram"));
        assert!(d.linked_user("discord", "100").is_some());
        // A new code drops every platform's link.
        d.set_link("u1", "Alice", "telegram", link("7"));
        d.assign_code("u1", "Alice", "c2".into(), 2);
        assert!(d.users["u1"].links.is_empty());
    }

    #[test]
    fn store_persists_and_survives_a_restart() {
        let dir = temp_dir();
        let store = Store::open(&dir).unwrap();
        let mac = store.code_mac("u1", "1234");
        assert_ne!(mac, store.code_mac("u2", "1234"), "codes are per user");
        let up = store.update(|d| {
            d.assign_code("u1", "Alice", mac.clone(), 1);
            d.settings.discord.guild_id = "42".into();
        });
        assert!(up.saved.is_ok());

        let raw = std::fs::read_to_string(dir.join(DATA_FILE)).unwrap();
        assert!(!raw.contains("1234"), "the code itself is never written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for f in [DATA_FILE, KEY_FILE] {
                let mode = std::fs::metadata(dir.join(f)).unwrap().permissions().mode();
                assert_eq!(mode & 0o077, 0, "{} is private", f);
            }
        }

        drop(store);
        let again = Store::open(&dir).unwrap();
        assert_eq!(again.code_mac("u1", "1234"), mac, "same key after restart");
        again.read(|d| {
            assert_eq!(d.users["u1"].code_hmac.as_deref(), Some(mac.as_str()));
            assert_eq!(d.settings.discord.guild_id, "42");
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_leftover_temp_file_does_not_widen_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let tmp = dir.join(DATA_FILE).with_extension("json.tmp");
        std::fs::write(&tmp, "stale").unwrap();
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644)).unwrap();
        let _store = Store::open(&dir).unwrap();
        let mode = std::fs::metadata(dir.join(DATA_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "data file is private");
        assert!(!tmp.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_corrupt_data_file_is_left_alone() {
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(DATA_FILE), "{ not json").unwrap();
        assert!(Store::open(&dir).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join(DATA_FILE)).unwrap(),
            "{ not json"
        );
        std::fs::write(dir.join(DATA_FILE), r#"{"version": 99}"#).unwrap();
        assert!(Store::open(&dir).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_bad_key_file_is_an_error() {
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(KEY_FILE), "short").unwrap();
        assert!(Store::open(&dir).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
