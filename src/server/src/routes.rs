use crate::auth::JwtConfig;
use crate::types::{Clients, Rooms};
use log::warn;
use std::collections::HashMap;
use std::sync::Arc;
use warp::Filter;

#[derive(Debug)]
struct OriginRejected;
impl warp::reject::Reject for OriginRejected {}

pub fn get_allowed_origins() -> Vec<String> {
    std::env::var("ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "http://localhost:8096,https://localhost:8096".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn is_origin_allowed(origin: &str, allowed: &Arc<Vec<String>>) -> bool {
    if allowed.iter().any(|o| o == "*") {
        warn!("SECURITY: Wildcard origin (*) configured - ALL origins allowed. This disables CORS protection!");
        return true;
    }
    allowed.iter().any(|o| o == origin)
}

pub fn build_ws_route(
    clients: Clients,
    rooms: Rooms,
    jwt_config: Arc<JwtConfig>,
    allowed_origins: Arc<Vec<String>>,
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    let clients_filter = warp::any().map(move || clients.clone());
    let rooms_filter = warp::any().map(move || rooms.clone());
    let jwt_filter = {
        let config = jwt_config;
        warp::any().map(move || config.clone())
    };
    let allowed_origins_filter = {
        let origins = allowed_origins;
        warp::any().map(move || origins.clone())
    };

    let origin_check = warp::header::optional::<String>("origin")
        .and(allowed_origins_filter)
        .and_then(
            |origin: Option<String>, allowed: Arc<Vec<String>>| async move {
                match origin {
                    Some(ref o) if is_origin_allowed(o, &allowed) => Ok(()),
                    Some(o) => {
                        warn!("Rejected connection from origin: {}", o);
                        Err(warp::reject::custom(OriginRejected))
                    }
                    None => Ok(()),
                }
            },
        )
        .untuple_one();

    let client_id_filter = warp::query::<HashMap<String, String>>()
        .map(|q: HashMap<String, String>| q.get("client_id").cloned());

    warp::path("ws")
        .and(origin_check)
        .and(warp::ws())
        .and(clients_filter)
        .and(rooms_filter)
        .and(jwt_filter)
        .and(client_id_filter)
        .map(
            |ws: warp::ws::Ws,
             clients,
             rooms,
             jwt_config: Arc<JwtConfig>,
             client_id: Option<String>| {
                ws.on_upgrade(move |socket| {
                    crate::ws::client_connection(socket, clients, rooms, jwt_config, client_id)
                })
            },
        )
}

pub fn build_health_route(
    jwt_config: Arc<JwtConfig>,
    allowed_origins: Arc<Vec<String>>,
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    let jwt_filter = warp::any().map(move || jwt_config.clone());

    let cors = if allowed_origins.iter().any(|o| o == "*") {
        warp::cors()
            .allow_any_origin()
            .allow_methods(vec!["GET"])
            .allow_headers(vec!["content-type"])
    } else {
        warp::cors()
            .allow_origins(
                allowed_origins
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>(),
            )
            .allow_methods(vec!["GET"])
            .allow_headers(vec!["content-type"])
    };

    warp::path("health")
        .and(warp::get())
        .and(jwt_filter)
        .map(|jwt_config: Arc<JwtConfig>| {
            warp::reply::json(&serde_json::json!({
                "status": "ok",
                "auth_enabled": jwt_config.enabled
            }))
        })
        .with(cors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_origin_allowed_exact_match() {
        let allowed = Arc::new(vec!["https://example.com".to_string()]);
        assert!(is_origin_allowed("https://example.com", &allowed));
    }

    #[test]
    fn is_origin_allowed_no_match() {
        let allowed = Arc::new(vec!["https://example.com".to_string()]);
        assert!(!is_origin_allowed("https://other.com", &allowed));
    }

    #[test]
    fn is_origin_allowed_wildcard() {
        let allowed = Arc::new(vec!["*".to_string()]);
        assert!(is_origin_allowed("https://anything.com", &allowed));
    }

    #[test]
    fn is_origin_allowed_empty_list() {
        let allowed = Arc::new(vec![]);
        assert!(!is_origin_allowed("https://example.com", &allowed));
    }

    #[test]
    fn is_origin_allowed_multiple_origins() {
        let allowed = Arc::new(vec![
            "https://a.com".to_string(),
            "https://b.com".to_string(),
        ]);
        assert!(is_origin_allowed("https://b.com", &allowed));
        assert!(!is_origin_allowed("https://c.com", &allowed));
    }

    #[test]
    fn get_allowed_origins_default() {
        let result = get_allowed_origins();
        assert!(!result.is_empty());
        for origin in &result {
            assert_eq!(origin, origin.trim());
            assert!(!origin.is_empty());
        }
    }
}
