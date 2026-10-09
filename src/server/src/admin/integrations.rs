//! Admin API for chat integrations: settings per platform, link codes for
//! Jellyfin users, and the integration's activity log.

use super::{error_response, AdminState};
use crate::integration::store::ChatSettings;
use crate::integration::{Integration, IntegrationStatus};
use crate::jellyfin::api::normalize_id;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use log::info;
use std::collections::HashSet;

/// Why chat integrations can't be used (answered as 409).
struct Unavailable(String);

impl IntoResponse for Unavailable {
    fn into_response(self) -> Response {
        error_response(StatusCode::CONFLICT, &self.0)
    }
}

fn hub(state: &AdminState) -> Result<&Integration, Unavailable> {
    match &state.integration {
        IntegrationStatus::Enabled(i) => Ok(i),
        IntegrationStatus::Unavailable(reason) => Err(Unavailable(reason.clone())),
    }
}

pub async fn status(State(state): State<AdminState>) -> Response {
    match &state.integration {
        IntegrationStatus::Unavailable(reason) => Json(serde_json::json!({
            "available": false,
            "reason": reason,
        }))
        .into_response(),
        IntegrationStatus::Enabled(i) => Json(i.status_json()).into_response(),
    }
}

pub async fn save_settings(
    State(state): State<AdminState>,
    Path(provider): Path<String>,
    Json(settings): Json<ChatSettings>,
) -> Response {
    let hub = match hub(&state) {
        Ok(h) => h,
        Err(r) => return r.into_response(),
    };
    let settings = match settings.validate() {
        Ok(s) => s,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e),
    };
    let up = hub
        .store()
        .update(|d| match d.settings.for_provider_mut(&provider) {
            Some(slot) => {
                *slot = settings.clone();
                true
            }
            None => false,
        });
    if !up.value {
        return error_response(StatusCode::NOT_FOUND, "Unknown platform");
    }
    if let Err(e) = up.saved {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Not saved: {}", e),
        );
    }
    hub.settings_changed();
    hub.audit(
        "settings",
        "admin".into(),
        format!(
            "{} settings saved (enabled: {})",
            provider, settings.enabled
        ),
        false,
    );
    Json(serde_json::json!({ "ok": true })).into_response()
}

/// Jellyfin users with their code and linked accounts. Users that only
/// remain in the data file (deleted from Jellyfin) are listed as `missing`.
pub async fn users(State(state): State<AdminState>) -> Response {
    let hub = match hub(&state) {
        Ok(h) => h,
        Err(r) => return r.into_response(),
    };
    let (jf_users, error) = match hub.users().await {
        Ok(u) => (u.as_ref().clone(), None),
        Err(e) => (Vec::new(), Some(e)),
    };
    let rows = hub.store().read(|d| {
        let mut seen = HashSet::new();
        let mut rows: Vec<serde_json::Value> = Vec::new();
        let row = |id: &str, name: &str, is_admin: bool, disabled: bool, missing: bool| {
            let rec = d.users.get(id);
            serde_json::json!({
                "id": id,
                "name": name,
                "is_admin": is_admin,
                "disabled": disabled,
                "missing": missing,
                "code": rec.and_then(|r| r.code_hmac.as_ref().map(|_| serde_json::json!({
                    "assigned_at": r.assigned_at,
                    "failed": r.failed,
                    "frozen": r.frozen,
                }))),
                "links": rec.map(|r| &r.links),
            })
        };
        for u in &jf_users {
            seen.insert(u.id.clone());
            rows.push(row(&u.id, &u.name, u.is_admin(), u.is_disabled(), false));
        }
        if error.is_none() {
            for (id, r) in &d.users {
                if !seen.contains(id) {
                    rows.push(row(id, &r.name, false, false, true));
                }
            }
        } else {
            for (id, r) in &d.users {
                rows.push(row(id, &r.name, false, false, false));
            }
        }
        rows
    });
    let mut rows = rows;
    rows.sort_by(|a, b| {
        a["name"]
            .as_str()
            .map(str::to_lowercase)
            .cmp(&b["name"].as_str().map(str::to_lowercase))
    });
    Json(serde_json::json!({ "users": rows, "error": error })).into_response()
}

pub async fn assign_code(State(state): State<AdminState>, Path(id): Path<String>) -> Response {
    let hub = match hub(&state) {
        Ok(h) => h,
        Err(r) => return r.into_response(),
    };
    match hub.assign_code(&id).await {
        Ok(code) => {
            info!("admin: assigned a link code to user {}", normalize_id(&id));
            Json(serde_json::json!({ "ok": true, "code": code })).into_response()
        }
        Err(e) => error_response(StatusCode::CONFLICT, &e),
    }
}

pub async fn revoke_code(State(state): State<AdminState>, Path(id): Path<String>) -> Response {
    let hub = match hub(&state) {
        Ok(h) => h,
        Err(r) => return r.into_response(),
    };
    let id = normalize_id(&id);
    let up = hub.store().update(|d| {
        let name = d.users.get(&id).map(|r| r.name.clone());
        d.revoke_code(&id).then_some(name).flatten()
    });
    let Some(name) = up.value else {
        return error_response(StatusCode::NOT_FOUND, "This user has no code");
    };
    if let Err(e) = up.saved {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Not saved: {}", e),
        );
    }
    hub.audit(
        "code_revoke",
        "admin".into(),
        format!("removed the code and links of {}", name),
        false,
    );
    Json(serde_json::json!({ "ok": true })).into_response()
}

pub async fn unlink(
    State(state): State<AdminState>,
    Path((id, provider)): Path<(String, String)>,
) -> Response {
    let hub = match hub(&state) {
        Ok(h) => h,
        Err(r) => return r.into_response(),
    };
    let id = normalize_id(&id);
    let up = hub.store().update(|d| {
        let name = d.users.get(&id).map(|r| r.name.clone());
        d.unlink(&id, &provider).then_some(name).flatten()
    });
    let Some(name) = up.value else {
        return error_response(StatusCode::NOT_FOUND, "Not linked");
    };
    if let Err(e) = up.saved {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Not saved: {}", e),
        );
    }
    hub.audit(
        "unlink",
        "admin".into(),
        format!("unlinked the {} account of {}", provider, name),
        false,
    );
    Json(serde_json::json!({ "ok": true })).into_response()
}

pub async fn audit(State(state): State<AdminState>) -> Response {
    let hub = match hub(&state) {
        Ok(h) => h,
        Err(r) => return r.into_response(),
    };
    Json(serde_json::json!({ "entries": hub.audit_log() })).into_response()
}
