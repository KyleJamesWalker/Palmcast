use std::collections::HashMap;

use axum::Router;
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use qrcode::QrCode;
use qrcode::render::svg;
use serde::{Deserialize, Serialize};

use crate::assets::{self, Web};
use crate::session::Registry;
use crate::ws::{self, Join};

const MAX_DECK_BYTES: usize = 256 * 1024;
/// A deck is the largest thing anyone posts. The margin covers the JSON frame.
const MAX_BODY_BYTES: usize = MAX_DECK_BYTES + 4096;
/// A socket frame only ever carries a short command, so the default megabytes
/// are room a client does not need and an attacker would.
const MAX_WS_MESSAGE: usize = 16 * 1024;

/// A deck is somebody else's Markdown rendered on everybody's phone, so the
/// page is pinned to its own origin as well as escaped at the source.
const CSP: &str = "default-src 'self'; img-src 'self' data:; style-src 'self'; \
script-src 'self'; connect-src 'self' ws: wss:; frame-ancestors 'none'; \
base-uri 'none'; form-action 'self'; object-src 'none'";

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CSP),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

pub fn router(registry: Registry) -> Router {
    Router::new()
        .route("/", get(|| async { page("new.html") }))
        .route("/healthz", get(health))
        .route("/api/sessions", post(create_session))
        .route("/api/sessions/{id}", put(update_session))
        .route("/api/sessions/{id}/markdown", get(get_markdown))
        .route("/s/{id}", get(|| async { page("watch.html") }))
        .route("/s/{id}/stage", get(|| async { page("stage.html") }))
        .route("/s/{id}/present", get(|| async { page("present.html") }))
        .route("/s/{id}/qr.svg", get(qr))
        .route("/s/{id}/ws", get(socket))
        .fallback(asset)
        .layer(middleware::from_fn(security_headers))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(registry)
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    sessions: usize,
    viewers: usize,
}

/// Counts only. A probe has no business knowing what any room holds.
async fn health(State(registry): State<Registry>) -> Response {
    axum::Json(Health {
        status: "ok",
        sessions: registry.len(),
        viewers: registry.viewers(),
    })
    .into_response()
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
    let Some((id, token)) = registry.create(&body.markdown) else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "this instance is holding as many sessions as it can",
        )
            .into_response();
    };
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
    upgrade
        .max_message_size(MAX_WS_MESSAGE)
        .max_frame_size(MAX_WS_MESSAGE)
        .on_upgrade(move |sock| ws::serve(sock, registry, join))
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

async fn asset(uri: Uri, headers: HeaderMap) -> Response {
    serve(uri.path().trim_start_matches('/'), Some(&headers))
}

fn page(path: &str) -> Response {
    serve(path, None)
}

/// Assets change whenever the binary does, and a viewer who reloads after a
/// redeploy must not keep running the code from before it. `no-cache` makes the
/// browser revalidate every time, and the ETag keeps that revalidation a 304
/// rather than a fresh download.
fn serve(path: &str, request: Option<&HeaderMap>) -> Response {
    let Some(file) = Web::get(path) else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };

    let tag = format!("\"{}\"", hex(&file.metadata.sha256_hash()[..8]));
    if let Some(headers) = request
        && headers
            .get(header::IF_NONE_MATCH)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.split(',').any(|candidate| candidate.trim() == tag))
    {
        return (StatusCode::NOT_MODIFIED, [(header::ETAG, tag)]).into_response();
    }

    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let rewrite = path.ends_with(".html") || path.ends_with(".js");
    let body: Vec<u8> = if rewrite {
        match std::str::from_utf8(&file.data) {
            Ok(text) => assets::versioned(text).into_bytes(),
            Err(_) => file.data.to_vec(),
        }
    } else {
        file.data.to_vec()
    };

    (
        [
            (header::CONTENT_TYPE, mime.as_ref().to_string()),
            (header::CACHE_CONTROL, "no-cache".to_string()),
            (header::ETAG, tag),
        ],
        body,
    )
        .into_response()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
