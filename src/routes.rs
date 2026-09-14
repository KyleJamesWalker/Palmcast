use std::collections::HashMap;

use axum::Router;
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use qrcode::QrCode;
use qrcode::render::svg;
use serde::{Deserialize, Serialize};

use crate::assets::{self, Web};
use crate::deck;
use crate::images;
use crate::origin;
use crate::session::{EditError, Registry, Role, TalkError};
use crate::share;
use crate::ws::{self, Join};

pub const MAX_DECK_BYTES: usize = 256 * 1024;
/// A deck is the largest thing anyone posts. The margin covers the JSON frame.
const MAX_BODY_BYTES: usize = MAX_DECK_BYTES + 4096;
/// A socket frame only ever carries a short command, so the default megabytes
/// are room a client does not need and an attacker would.
const MAX_WS_MESSAGE: usize = 16 * 1024;

/// A deck is somebody else's Markdown rendered on everybody's phone, so the
/// page is pinned to its own origin as well as escaped at the source.
const CSP: &str = "default-src 'self'; img-src 'self' data: https: http:; \
style-src 'self'; script-src 'self'; connect-src 'self' ws: wss:; \
frame-ancestors 'none'; base-uri 'none'; form-action 'self'; object-src 'none'";

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

#[derive(Clone)]
pub struct App {
    pub registry: Registry,
    /// Set when the instance knows its own address. Without it the Host header
    /// decides, which is fine on a laptop and guesswork behind a proxy.
    pub public_url: Option<String>,
    /// The deck the start page opens with, for an instance that runs the same
    /// talk or quiz every time. Unset leaves the page its built-in sample.
    pub starter: Option<String>,
    /// Whether this instance keeps pictures for the rooms it serves. Off unless
    /// the operator turned it on, because it is the one feature here that holds
    /// bytes somebody else chose.
    pub uploads: bool,
}

impl axum::extract::FromRef<App> for Registry {
    fn from_ref(app: &App) -> Registry {
        app.registry.clone()
    }
}

pub fn router(registry: Registry) -> Router {
    router_with(App {
        registry,
        public_url: None,
        starter: None,
        uploads: false,
    })
}

pub fn router_with(app: App) -> Router {
    Router::new()
        .route("/", get(|| async { page("new.html") }))
        .route("/healthz", get(health))
        .route("/api/sessions", post(create_session))
        .route("/api/preview", post(preview_deck))
        .route("/api/starter", get(starter_deck))
        .route("/api/config", get(config))
        .route(
            "/api/sessions/{id}/images",
            post(upload_image).layer(DefaultBodyLimit::max(images::MAX_UPLOAD_BYTES + 4096)),
        )
        .route("/i/{id}/{image}", get(serve_image))
        .route("/api/pack", post(pack_deck))
        .route("/api/unpack", post(unpack_deck))
        .route(
            "/api/sessions/{id}",
            get(session_exists).put(update_session),
        )
        .route("/api/sessions/{id}/markdown", get(get_markdown))
        .route("/api/sessions/{id}/talks", post(submit_talk))
        .route(
            "/api/sessions/{id}/talks/{talk}",
            get(read_talk).put(update_talk),
        )
        .route("/api/sessions/{id}/export", get(export_evening))
        .route("/api/sessions/{id}/cohost", get(cohost_link))
        .route("/api/sessions/{id}/role", get(whoami))
        .route("/s/{id}", get(|| async { page("watch.html") }))
        .route("/s/{id}/stage", get(|| async { page("stage.html") }))
        .route("/s/{id}/present", get(|| async { page("present.html") }))
        .route("/s/{id}/qr.svg", get(qr))
        .route("/s/{id}/ws", get(socket))
        .fallback(asset)
        .layer(middleware::from_fn(security_headers))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(app)
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

/// The deck this instance was started with, if it was started with one.
///
/// 204 rather than an empty body, because the start page has its own sample to
/// fall back on and "no deck configured" is not "a deck of nothing".
async fn starter_deck(State(app): State<App>) -> Response {
    match app.starter {
        Some(markdown) => (
            [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
            markdown,
        )
            .into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

#[derive(Deserialize, Serialize)]
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

/// The whole evening as a zip: every deck, what the room asked, and a cue file
/// timed against a recording. Host only, because it carries every talk.
async fn export_evening(
    State(registry): State<Registry>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let token = params.get("token").map(String::as_str).unwrap_or_default();
    let built = registry.with(&id, |s| {
        s.role_of(token).hosts().then(|| crate::export::bundle(s))
    });
    match built {
        Some(Some(Ok(bytes))) => (
            [
                (header::CONTENT_TYPE, "application/zip".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"palmcast-{id}.zip\""),
                ),
            ],
            bytes,
        )
            .into_response(),
        Some(Some(Err(_))) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        Some(None) => StatusCode::FORBIDDEN.into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Deserialize)]
struct TalkBody {
    #[serde(default)]
    title: String,
    markdown: String,
    /// The browser id, the same one the socket uses, so a submission is
    /// attributed to whoever is already in the room.
    who: String,
}

#[derive(Serialize)]
struct Submitted {
    id: u64,
    token: String,
}

/// Adds a talk to the running order.
///
/// Open to the room, because that is the point, and bounded because of it: the
/// room has to be taking submissions, and the caps sit in the session.
async fn submit_talk(
    State(registry): State<Registry>,
    Path(id): Path<String>,
    axum::Json(body): axum::Json<TalkBody>,
) -> Response {
    if body.markdown.len() > MAX_DECK_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, "deck too large").into_response();
    }
    let Some(outcome) =
        registry.with_mut(&id, |s| s.submit(&body.who, &body.title, &body.markdown))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match outcome {
        Some((talk, token)) => (
            StatusCode::CREATED,
            axum::Json(Submitted { id: talk, token }),
        )
            .into_response(),
        None => (
            StatusCode::CONFLICT,
            "this room is not taking talks right now",
        )
            .into_response(),
    }
}

/// One submitted talk, whole: for the host reading it before putting it up, and
/// for the speaker who wrote it checking it over while they wait.
///
/// The running order is public and the decks behind it are not, so this answers
/// two tokens and no others. The host's note travels here rather than on the
/// socket, because it is for one speaker and a broadcast reaches a room.
async fn read_talk(
    State(registry): State<Registry>,
    Path((id, talk)): Path<(String, u64)>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let token = params.get("token").map(String::as_str).unwrap_or_default();
    let found = registry.with(&id, |s| match s.talk_detail(talk) {
        None => Err(StatusCode::NOT_FOUND),
        Some(_) if !allowed(s, talk, token) => Err(StatusCode::FORBIDDEN),
        Some(detail) => Ok(detail),
    });
    match found {
        Some(Ok(detail)) => axum::Json(detail).into_response(),
        Some(Err(status)) => status.into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// The two tokens a talk answers to: the host's, and the one minted for the
/// talk itself.
fn allowed(session: &crate::session::Session, talk: u64, token: &str) -> bool {
    session.role_of(token).edits() || session.owns_talk(talk, token)
}

#[derive(Deserialize)]
struct TalkEdit {
    #[serde(default)]
    title: String,
    markdown: String,
}

/// Rewrites a talk that is still waiting.
///
/// The speaker keeps their own deck until the room sees it: the whole point of
/// putting a talk up early is being able to fix it before you stand up. A talk
/// the host dropped comes back to the running order on the save that fixes it.
async fn update_talk(
    State(registry): State<Registry>,
    Path((id, talk)): Path<(String, u64)>,
    Query(params): Query<HashMap<String, String>>,
    axum::Json(body): axum::Json<TalkEdit>,
) -> Response {
    let token = params.get("token").map(String::as_str).unwrap_or_default();
    let outcome = registry.with_mut(&id, |s| {
        // A talk that is gone is gone for everyone, so say so rather than
        // refusing the speaker who wrote it.
        if s.talk_detail(talk).is_none() {
            return Some(Err(TalkError::Gone));
        }
        if !allowed(s, talk, token) {
            return None;
        }
        Some(s.update_talk(talk, &body.title, &body.markdown))
    });
    match outcome {
        Some(Some(Ok(()))) => StatusCode::NO_CONTENT.into_response(),
        Some(Some(Err(TalkError::TooLarge))) => {
            (StatusCode::PAYLOAD_TOO_LARGE, "that talk is too long").into_response()
        }
        Some(Some(Err(TalkError::Staged))) => (
            StatusCode::CONFLICT,
            "the room is looking at this talk right now",
        )
            .into_response(),
        Some(Some(Err(TalkError::Gone))) | None => StatusCode::NOT_FOUND.into_response(),
        Some(None) => StatusCode::FORBIDDEN.into_response(),
    }
}

#[derive(Serialize)]
struct Config {
    uploads: bool,
}

/// What this instance lets a view offer. A page that cannot upload should not
/// show a button that fails.
async fn config(State(app): State<App>) -> Response {
    axum::Json(Config {
        uploads: app.uploads,
    })
    .into_response()
}

#[derive(Serialize)]
struct Uploaded {
    url: String,
}

/// Takes a picture for one room.
///
/// Open to whoever may write a deck here: the host, a co-host, a speaker with
/// their own talk, and the room itself while it is taking talks. Bytes are
/// decoded and shrunk before anything is kept, so what the room serves is
/// never what a phone camera produced.
async fn upload_image(
    State(app): State<App>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if !app.uploads {
        return (StatusCode::NOT_FOUND, "this instance does not keep images").into_response();
    }
    let declared = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !images::readable_type(declared) {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "that is not an image").into_response();
    }

    let token = params.get("token").map(String::as_str).unwrap_or_default();
    let who = params.get("who").map(String::as_str).unwrap_or_default();
    let talk = params.get("talk").and_then(|t| t.parse::<u64>().ok());

    let allowed = app.registry.with(&id, |s| {
        s.role_of(token).edits()
            || talk.is_some_and(|talk| s.owns_talk(talk, token))
            || s.takes_talks()
    });
    match allowed {
        None => return StatusCode::NOT_FOUND.into_response(),
        Some(false) => return StatusCode::FORBIDDEN.into_response(),
        Some(true) => {}
    }

    // Off the runtime's thread: decoding and resizing a photograph is work, and
    // every other room on this instance is waiting on the same executor.
    let shrunk = tokio::task::spawn_blocking(move || images::shrink(&body)).await;
    let (bytes, kind) = match shrunk {
        Ok(Ok(out)) => out,
        Ok(Err(error)) => {
            let status = match error {
                images::ImageError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
                images::ImageError::Unreadable => StatusCode::BAD_REQUEST,
            };
            return (status, error.message()).into_response();
        }
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let stored = app
        .registry
        .with_mut(&id, |s| s.store_image(who, bytes, kind))
        .flatten();
    match stored {
        Some(image) => (
            StatusCode::CREATED,
            axum::Json(Uploaded {
                url: format!("/i/{id}/{image}"),
            }),
        )
            .into_response(),
        None => (
            StatusCode::TOO_MANY_REQUESTS,
            "this room is holding as many images as it can, or that was too soon",
        )
            .into_response(),
    }
}

/// Serves one picture. The id is random and the bytes never change under it, so
/// a phone that has drawn it once never asks again.
async fn serve_image(
    State(registry): State<Registry>,
    Path((id, image)): Path<(String, String)>,
) -> Response {
    let found = registry.with(&id, |s| {
        s.image(&image).map(|held| (held.kind, held.bytes.clone()))
    });
    match found {
        Some(Some((kind, bytes))) => (
            [
                (header::CONTENT_TYPE, kind.to_string()),
                (
                    header::CACHE_CONTROL,
                    "public, max-age=31536000, immutable".to_string(),
                ),
            ],
            bytes,
        )
            .into_response(),
        _ => (StatusCode::NOT_FOUND, "no such image").into_response(),
    }
}

#[derive(Serialize)]
struct Preview {
    slides: Vec<deck::Slide>,
}

/// Renders a deck without starting a room.
///
/// The same parser the room runs, so what the author reads here is what the
/// audience gets. A preview that rendered Markdown separately would be a second
/// place for the sanitizer to be wrong.
async fn preview_deck(axum::Json(body): axum::Json<DeckBody>) -> Response {
    if body.markdown.len() > MAX_DECK_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, "deck too large").into_response();
    }
    axum::Json(Preview {
        slides: deck::parse(&body.markdown),
    })
    .into_response()
}

#[derive(Serialize)]
struct Packed {
    token: String,
}

#[derive(Deserialize)]
struct TokenBody {
    token: String,
}

/// Turns a deck into the token half of a share link.
///
/// The deck arrives in a body rather than a query string so it stays out of
/// access logs, and no session has to exist: a deck is shareable before it is
/// ever presented.
async fn pack_deck(axum::Json(body): axum::Json<DeckBody>) -> Response {
    match share::pack(&body.markdown) {
        Ok(token) => axum::Json(Packed { token }).into_response(),
        Err(error) => (StatusCode::PAYLOAD_TOO_LARGE, error.to_string()).into_response(),
    }
}

/// The other direction. The token lives in the URL fragment on the client, so
/// posting it back is what keeps a shared deck out of this server's logs.
async fn unpack_deck(axum::Json(body): axum::Json<TokenBody>) -> Response {
    match share::unpack(&body.token) {
        Ok(markdown) => axum::Json(DeckBody { markdown }).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
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
    let base_rev = params.get("rev").and_then(|r| r.parse::<u64>().ok());

    let role = registry.role(&id, token);
    // The save and the broadcast that announces it happen under one lock, so
    // the room cannot be told about revisions out of the order they landed.
    let outcome = registry
        .with_mut(&id, |s| s.replace_deck(role, base_rev, &body.markdown))
        .unwrap_or(Err(EditError::Gone));
    match outcome {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(EditError::Forbidden) => StatusCode::FORBIDDEN.into_response(),
        Err(EditError::Gone) => StatusCode::NOT_FOUND.into_response(),
        // The other editor got there first. The revision to rebase on comes
        // back so the client can say so rather than silently losing the work.
        Err(EditError::Stale { current }) => (
            StatusCode::CONFLICT,
            [(header::ETAG, format!("\"{current}\""))],
            "the deck changed while you were editing",
        )
            .into_response(),
    }
}

/// Lets a console show the controls its token actually works.
async fn whoami(
    State(registry): State<Registry>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let token = params.get("token").map(String::as_str).unwrap_or_default();
    // A driver drives without editing, so it cannot fold into either of the
    // other two: the console has to hide the lineup from a speaker.
    let name = match registry.role(&id, token) {
        Role::Mc => "mc",
        Role::CoHost => "cohost",
        Role::Driver => "driver",
        Role::Viewer => "viewer",
    };
    name.into_response()
}

/// Hands the MC a token that edits but does not drive.
async fn cohost_link(
    State(registry): State<Registry>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let token = params.get("token").map(String::as_str).unwrap_or_default();
    match registry.cohost_token(&id, token) {
        Some(cohost) => cohost.into_response(),
        None => StatusCode::FORBIDDEN.into_response(),
    }
}

async fn session_exists(State(registry): State<Registry>, Path(id): Path<String>) -> Response {
    if registry.exists(&id) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

async fn get_markdown(
    State(registry): State<Registry>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let token = params.get("token").map(String::as_str).unwrap_or_default();
    if !registry.role(&id, token).edits() {
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

async fn qr(State(app): State<App>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    if !app.registry.exists(&id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let header_str = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let url = origin::audience_url(
        app.public_url.as_deref(),
        header_str("host").as_deref(),
        header_str("x-forwarded-proto").as_deref(),
        &id,
    );

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

    // Html and js are rewritten to carry the build id, so the bytes sent depend
    // on every asset and not just this one. An etag over the file alone lets a
    // cached client answer a revalidation with a body holding last build's
    // import urls, which is the staleness the versioning exists to prevent.
    let rewritten = path.ends_with(".html") || path.ends_with(".js");
    let digest = hex(&file.metadata.sha256_hash()[..8]);
    let tag = if rewritten {
        format!("\"{digest}-{}\"", assets::build_id())
    } else {
        format!("\"{digest}\"")
    };
    if let Some(headers) = request
        && headers
            .get(header::IF_NONE_MATCH)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.split(',').any(|candidate| candidate.trim() == tag))
    {
        return (StatusCode::NOT_MODIFIED, [(header::ETAG, tag)]).into_response();
    }

    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let body: Vec<u8> = if rewritten {
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
