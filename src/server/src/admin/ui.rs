//! The admin UI: three static files compiled into the binary. Plain
//! HTML/CSS/JS with no build step, like the Jellyfin web client. All URLs in
//! them are relative, so the panel also works under a reverse-proxy subpath.

use axum::http::header;
use axum::response::{IntoResponse, Response};

const INDEX_HTML: &str = include_str!("ui/index.html");
const APP_JS: &str = include_str!("ui/app.js");
const APP_CSS: &str = include_str!("ui/app.css");

pub async fn index() -> Response {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        INDEX_HTML,
    )
        .into_response()
}

pub async fn app_js() -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        APP_JS,
    )
        .into_response()
}

pub async fn app_css() -> Response {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], APP_CSS).into_response()
}
