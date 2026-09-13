use std::collections::HashMap;

use axum::Router;
use axum::extract::{Path, Query, State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use qrcode::QrCode;
use qrcode::render::svg;
use serde::{Deserialize, Serialize};

use crate::assets::Web;
use crate::session::Registry;
use crate::ws::{self, Join};

const MAX_DECK_BYTES: usize = 256 * 1024;

pub fn router(registry: Registry) -> Router {
    Router::new()
        .route("/", get(|| async { page("new.html") }))
        .route("/api/sessions", post(create_session))
        .route("/api/sessions/{id}", put(update_session))
        .route("/api/sessions/{id}/markdown", get(get_markdown))
        .route("/s/{id}", get(|| async { page("watch.html") }))
        .route("/s/{id}/stage", get(|| async { page("stage.html") }))
        .route("/s/{id}/present", get(|| async { page("present.html") }))
        .route("/s/{id}/qr.svg", get(qr))
        .route("/s/{id}/ws", get(socket))
        .fallback(asset)
        .with_state(registry)
}

#[derive(Deserialize)]
struct DeckBody {
    markdown: String,
}

#[derive(Serialize)]
struct Created {
    id: String,
    token: String,
}

async fn create_session(
    State(registry): State<Registry>,
    axum::Json(body): axum::Json<DeckBody>,
) -> Response {
    if body.markdown.len() > MAX_DECK_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, "deck too large").into_response();
    }
    let (id, token) = registry.create(&body.markdown);
    (StatusCode::CREATED, axum::Json(Created { id, token })).into_response()
}

async fn update_session(
    State(registry): State<Registry>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    axum::Json(body): axum::Json<DeckBody>,
) -> Response {
    if body.markdown.len() > MAX_DECK_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, "deck too large").into_response();
    }
    let token = params.get("token").map(String::as_str).unwrap_or_default();
    match registry.replace_deck(&id, token, &body.markdown) {
        Some(snapshot) => {
            registry.broadcast(&id, snapshot);
            StatusCode::NO_CONTENT.into_response()
        }
        None => StatusCode::FORBIDDEN.into_response(),
    }
}

async fn get_markdown(
    State(registry): State<Registry>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let token = params.get("token").map(String::as_str).unwrap_or_default();
    if !registry.owns(&id, token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match registry.markdown(&id) {
        Some(markdown) => markdown.into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn socket(
    State(registry): State<Registry>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !registry.exists(&id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let join = Join {
        id,
        token: params.get("token").cloned(),
        who: params.get("who").cloned().unwrap_or_default(),
    };
    upgrade.on_upgrade(move |sock| ws::serve(sock, registry, join))
}

async fn qr(
    State(registry): State<Registry>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !registry.exists(&id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost");
    let scheme = if host.starts_with("localhost") || host.starts_with("127.") {
        "http"
    } else {
        "https"
    };
    let url = format!("{scheme}://{host}/s/{id}");

    let Ok(code) = QrCode::new(url.as_bytes()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let image = code
        .render()
        .min_dimensions(240, 240)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .build();
    ([(header::CONTENT_TYPE, "image/svg+xml")], image).into_response()
}

async fn asset(uri: Uri) -> Response {
    page(uri.path().trim_start_matches('/'))
}

fn page(path: &str) -> Response {
    match Web::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref())], file.data).into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}
