//! What chat users can do, and who may do it.
//!
//! Every action first passes `caller` (platform enabled, right server and
//! channel, required role, per-account rate limit), then - except linking -
//! `me` (the chat account is linked to an existing, enabled Jellyfin user).

use super::store::{clean_code, CodeCheck, Link};
use super::{clean_display_name, valid_external_id, Actor, IntError, IntResult, Integration};
use crate::jellyfin::api::normalize_id;
use crate::utils::now_ms;

const MAX_USERNAME_LEN: usize = 128;

/// The checked request context.
pub struct Caller {
    pub provider: String,
    pub actor: Actor,
    role_admin: bool,
}

impl Caller {
    fn label(&self) -> String {
        format!("{}:{} ({})", self.provider, self.actor.id, self.actor.name)
    }
}

/// A caller linked to a Jellyfin user.
pub struct Me {
    pub user_id: String,
    pub user_name: String,
    pub is_admin: bool,
}

/// Outcome of a link attempt, decided under the store lock.
enum LinkOutcome {
    Linked,
    Taken,
    Wrong { frozen_now: bool },
    Refused,
}

impl Integration {
    /// Checks the platform-side policy for a request.
    pub fn caller(&self, provider: &str, mut actor: Actor) -> Result<Caller, IntError> {
        if !valid_external_id(&actor.id) {
            return Err(IntError::invalid("Missing or invalid account id"));
        }
        actor.name = clean_display_name(&actor.name);
        if let Some(wait) = self
            .guards()
            .rate_limited(&format!("{}:{}", provider, actor.id), now_ms())
        {
            return Err(IntError::wait(
                "rate_limited",
                "Slow down a little and try again in a moment",
                wait,
            ));
        }
        let settings = self
            .store()
            .read(|d| d.settings.for_provider(provider).cloned())
            .ok_or_else(|| IntError::forbidden("not_configured", "Unknown platform"))?;
        if !settings.enabled {
            return Err(IntError::forbidden(
                "disabled",
                "Watch parties from chat are turned off on this server",
            ));
        }
        if settings.guild_id.is_empty() || settings.guild_id != actor.guild_id {
            return Err(IntError::forbidden(
                "wrong_guild",
                "This bot only works in its own server",
            ));
        }
        if !settings.channel_ids.is_empty() && !settings.channel_ids.contains(&actor.channel_id) {
            return Err(IntError::forbidden(
                "channel_not_allowed",
                "Use the watch party channel for this",
            ));
        }
        if !settings.required_role_id.is_empty()
            && !actor.roles.contains(&settings.required_role_id)
        {
            return Err(IntError::forbidden(
                "missing_role",
                "You need the watch party role to use this",
            ));
        }
        let role_admin =
            !settings.admin_role_id.is_empty() && actor.roles.contains(&settings.admin_role_id);
        Ok(Caller {
            provider: provider.to_string(),
            actor,
            role_admin,
        })
    }

    /// The linked, still existing and enabled Jellyfin user behind a caller.
    pub async fn me(&self, caller: Caller) -> Result<Me, IntError> {
        let (user_id, stored_name) = self
            .store()
            .read(|d| {
                d.linked_user(&caller.provider, &caller.actor.id)
                    .map(|(id, r)| (id.to_string(), r.name.clone()))
            })
            .ok_or_else(|| {
                IntError::forbidden(
                    "not_linked",
                    "Link your Jellyfin account first: use the link command with the code from your admin",
                )
            })?;
        let users = self.users().await.map_err(IntError::jellyfin)?;
        let user = users
            .iter()
            .find(|u| u.id == user_id && !u.is_disabled())
            .ok_or_else(|| {
                IntError::forbidden(
                    "account_disabled",
                    "Your Jellyfin account is disabled or gone; ask an admin",
                )
            })?;
        if user.name != stored_name {
            self.store().update(|d| {
                if let Some(r) = d.users.get_mut(&user_id) {
                    r.name = user.name.clone();
                }
            });
        }
        Ok(Me {
            is_admin: user.is_admin() || caller.role_admin,
            user_id,
            user_name: user.name.clone(),
        })
    }

    pub async fn me_for(&self, provider: &str, actor: Actor) -> Result<Me, IntError> {
        let caller = self.caller(provider, actor)?;
        self.me(caller).await
    }

    // --- linking -----------------------------------------------------------

    pub async fn link(
        &self,
        provider: &str,
        actor: Actor,
        username: &str,
        code: &str,
    ) -> IntResult {
        let caller = self.caller(provider, actor)?;
        let key = format!("{}:{}", provider, caller.actor.id);
        if let Some(wait) = self.guards().link_blocked(&key, now_ms()) {
            self.audit(
                "link_blocked",
                caller.label(),
                "link attempt while locked out".into(),
                true,
            );
            return Err(IntError::wait(
                "locked_out",
                format!(
                    "Too many wrong attempts. Try again in {} minutes",
                    wait.div_ceil(60_000)
                ),
                wait,
            ));
        }
        let users = self.users().await.map_err(IntError::jellyfin)?;
        let wanted = username.trim().to_lowercase();
        let target = (!wanted.is_empty() && wanted.chars().count() <= MAX_USERNAME_LEN)
            .then(|| {
                users
                    .iter()
                    .find(|u| u.name.to_lowercase() == wanted && !u.is_disabled())
            })
            .flatten();
        let code = clean_code(code);

        let outcome = match (target, code.as_deref()) {
            (Some(user), Some(code)) => {
                let mac = self.store().code_mac(&user.id, code);
                let link = Link {
                    external_id: caller.actor.id.clone(),
                    display_name: caller.actor.name.clone(),
                    linked_at: now_ms(),
                };
                self.store()
                    .update(|d| match d.check_code(&user.id, &mac) {
                        CodeCheck::Match => {
                            let taken = d
                                .link_of(&user.id, provider)
                                .is_some_and(|l| l.external_id != link.external_id);
                            if taken {
                                LinkOutcome::Taken
                            } else {
                                d.set_link(&user.id, &user.name, provider, link);
                                LinkOutcome::Linked
                            }
                        }
                        CodeCheck::Wrong { frozen_now } => LinkOutcome::Wrong { frozen_now },
                        CodeCheck::NoCode | CodeCheck::Frozen => LinkOutcome::Refused,
                    })
                    .value
            }
            _ => LinkOutcome::Refused,
        };
        let who = target.map(|u| u.name.as_str()).unwrap_or("an unknown user");
        match outcome {
            LinkOutcome::Linked => {
                let user = target.expect("linked implies a target");
                self.guards().clear_link_fails(&key);
                self.audit(
                    "link",
                    caller.label(),
                    format!("linked to {}", user.name),
                    false,
                );
                Ok(serde_json::json!({ "ok": true, "user_name": user.name }))
            }
            LinkOutcome::Taken => {
                self.audit(
                    "link_refused",
                    caller.label(),
                    format!(
                        "entered the right code for {}, who is linked to another account; consider reassigning the code",
                        who
                    ),
                    true,
                );
                Err(IntError::conflict(
                    "already_linked_elsewhere",
                    "That Jellyfin account is already linked to another account. Ask an admin to unlink it",
                ))
            }
            LinkOutcome::Wrong { frozen_now } => {
                self.guards().record_link_fail(&key, now_ms());
                self.audit(
                    "link_failed",
                    caller.label(),
                    format!("wrong code for {}", who),
                    true,
                );
                if frozen_now {
                    self.audit(
                        "code_frozen",
                        caller.label(),
                        format!(
                            "the code for {} stopped working after too many wrong attempts; assign a new one",
                            who
                        ),
                        true,
                    );
                }
                Err(bad_credentials())
            }
            LinkOutcome::Refused => {
                self.guards().record_link_fail(&key, now_ms());
                self.audit(
                    "link_failed",
                    caller.label(),
                    format!("refused for {} (unknown, disabled, no code or frozen)", who),
                    true,
                );
                Err(bad_credentials())
            }
        }
    }

    pub async fn unlink(&self, provider: &str, actor: Actor) -> IntResult {
        let caller = self.caller(provider, actor)?;
        let removed = self
            .store()
            .update(|d| {
                let id = d
                    .linked_user(provider, &caller.actor.id)
                    .map(|(id, _)| id.to_string());
                id.map(|id| d.unlink(&id, provider))
            })
            .value;
        if removed != Some(true) {
            return Err(IntError::forbidden(
                "not_linked",
                "Your account isn't linked",
            ));
        }
        self.audit(
            "unlink",
            caller.label(),
            "unlinked themselves".into(),
            false,
        );
        Ok(serde_json::json!({ "ok": true }))
    }

    pub async fn whoami(&self, provider: &str, actor: Actor) -> IntResult {
        let me = self.me_for(provider, actor).await?;
        Ok(serde_json::json!({
            "user_id": me.user_id,
            "user_name": me.user_name,
            "is_admin": me.is_admin,
        }))
    }

    // --- admin -------------------------------------------------------------

    /// Assigns a fresh code to a Jellyfin user (dropping their links).
    /// Returns the code: it is shown to the admin once and never stored.
    pub async fn assign_code(&self, user_id: &str) -> Result<String, String> {
        let user_id = normalize_id(user_id);
        let users = self.fresh_users().await?;
        let user = users
            .iter()
            .find(|u| u.id == user_id)
            .ok_or("No such Jellyfin user")?;
        let code = super::store::generate_code();
        let mac = self.store().code_mac(&user.id, &code);
        let saved = self
            .store()
            .update(|d| d.assign_code(&user.id, &user.name, mac, now_ms()))
            .saved;
        saved.map_err(|e| format!("Not saved: {}", e))?;
        self.audit(
            "code_assign",
            "admin".into(),
            format!("assigned a new code to {}", user.name),
            false,
        );
        Ok(code)
    }
}

fn bad_credentials() -> IntError {
    IntError::forbidden(
        "bad_credentials",
        "That username or code is wrong. Check them, or ask an admin for a new code",
    )
}
