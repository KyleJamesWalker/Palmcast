use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Query, Request, State, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use qrcode::QrCode;
use qrcode::render::svg;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

use crate::assets::{self, Web};
use crate::deck;
use crate::images;
use crate::origin;
use crate::session::{EditError, Registry, Role, TalkError};
use crate::share;
use crate::styles::{self, Look, Styles};
use crate::ws::{self, Heartbeat, Join};

pub const MAX_DECK_BYTES: usize = 256 * 1024;
/// A deck is the largest thing anyone posts. The margin covers the JSON frame.
const MAX_BODY_BYTES: usize = MAX_DECK_BYTES + 4096;
/// A socket frame only ever carries a short command, so the default megabytes
/// are room a client does not need and an attacker would.
const MAX_WS_MESSAGE: usize = 16 * 1024;
/// Pictures decoded at once across the instance. Each holds its full bitmap
/// while it is worked on, so this bounds the memory a burst of uploads costs.
const MAX_CONCURRENT_DECODES: usize = 2;

/// A deck is somebody else's Markdown rendered on everybody's phone, so the
/// page is pinned to its own origin as well as escaped at the source.
const CSP: &str = "default-src 'self'; img-src 'self' data: https: http:; \
style-src 'self'; script-src 'self'; connect-src 'self' ws: wss:; \
frame-ancestors 'none'; base-uri 'none'; form-action 'self'; object-src 'none'";

/// What a single address may do, and how often.
///
/// Creating a room costs the instance a slot out of its session cap for the
/// whole TTL, so it is the expensive one. Packing and previewing cost a parse
/// and a compress, so they are metered per minute instead.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Limit {
    Create,
    Pack,
}

impl Limit {
    fn refused(self) -> &'static str {
        match self {
            Limit::Create => "too many rooms from this address, try again later",
            Limit::Pack => "too many decks from this address, slow down",
        }
    }
}

struct Bucket {
    tokens: f64,
    seen: Instant,
}

/// A token bucket per address and limit. Hand rolled rather than a dependency,
/// because it is thirty lines and the lockfile is checked with `--locked`.
pub struct Limiter {
    buckets: Mutex<HashMap<(IpAddr, Limit), Bucket>>,
    /// Rooms one address may start in an hour.
    create_per_hour: f64,
    /// Decks one address may pack or preview in a minute.
    pack_per_minute: f64,
}

impl Default for Limiter {
    fn default() -> Self {
        Self::new(10, 60)
    }
}

/// Anything untouched for longer than the widest window is indistinguishable
/// from a full bucket, so it is dropped rather than remembered.
const BUCKET_TTL: Duration = Duration::from_secs(3600);

impl Limiter {
    pub fn new(create_per_hour: u32, pack_per_minute: u32) -> Self {
        Self {
            buckets: Mutex::default(),
            create_per_hour: create_per_hour.into(),
            pack_per_minute: pack_per_minute.into(),
        }
    }

    /// How many, and over what window.
    fn allowance(&self, limit: Limit) -> (f64, f64) {
        match limit {
            Limit::Create => (self.create_per_hour, 3600.0),
            Limit::Pack => (self.pack_per_minute, 60.0),
        }
    }

    /// True when the call may go ahead, taking one token if so.
    pub fn take(&self, who: IpAddr, limit: Limit) -> bool {
        let (capacity, window) = self.allowance(limit);
        let now = Instant::now();
        let mut buckets = self
            .buckets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        buckets.retain(|_, bucket| now.duration_since(bucket.seen) < BUCKET_TTL);

        let bucket = buckets.entry((who, limit)).or_insert(Bucket {
            tokens: capacity,
            seen: now,
        });
        let refill = now.duration_since(bucket.seen).as_secs_f64() * (capacity / window);
        bucket.tokens = (bucket.tokens + refill).min(capacity);
        bucket.seen = now;
        if bucket.tokens < 1.0 {
            return false;
        }
        bucket.tokens -= 1.0;
        true
    }
}

/// Who to meter. Behind a proxy the peer address is the proxy, so the forwarded
/// header is read instead — but only when `--public-url` says a proxy is there.
/// Trusting it otherwise would let anyone pick their own bucket.
fn client_ip(app: &App, headers: &HeaderMap, peer: SocketAddr) -> IpAddr {
    if app.public_url.is_some()
        && let Some(forwarded) = headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
        && let Some(first) = forwarded.split(',').next()
        && let Ok(ip) = first.trim().parse::<IpAddr>()
    {
        return ip;
    }
    peer.ip()
}

/// The caller's token: the Authorization header first, the query string second.
///
/// A reverse proxy logs the full request line, so a token in the query lands in
/// access logs. The query form is kept for one release so tabs opened before
/// the change keep working, and says so in the log when it is used.
fn token_from(headers: &HeaderMap, params: &HashMap<String, String>) -> String {
    if let Some(bearer) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    {
        return bearer.trim().to_string();
    }
    match params.get("token") {
        Some(token) => {
            tracing::warn!("token read from a query string; move it to the Authorization header");
            token.clone()
        }
        None => String::new(),
    }
}

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
    /// The themes and transitions a deck here may name. Read once at startup
    /// and shared, because every room in the room reads the same ones.
    pub styles: Arc<Styles>,
    /// Set to gate room creation on a key the operator hands out. Unset leaves
    /// the instance open to anyone who can reach it.
    pub create_key: Option<String>,
    /// Per address, so one visitor cannot take the whole session cap.
    pub limiter: Arc<Limiter>,
    /// How often a socket is pinged and how long it may say nothing. A test
    /// builds these short; nothing else has reason to change them.
    pub heartbeat: Heartbeat,
    /// How many pictures may be decoded at once across the whole instance.
    /// A decode holds the full bitmap, so this is the real memory ceiling.
    pub decoding: Arc<tokio::sync::Semaphore>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            registry: Registry::new(Duration::from_secs(6 * 3600)),
            public_url: None,
            starter: None,
            uploads: false,
            styles: Arc::default(),
            create_key: None,
            limiter: Arc::default(),
            heartbeat: Heartbeat::default(),
            decoding: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_DECODES)),
        }
    }
}

impl axum::extract::FromRef<App> for Registry {
    fn from_ref(app: &App) -> Registry {
        app.registry.clone()
    }
}

pub fn router(registry: Registry) -> Router {
    router_with(App {
        registry,
        styles: Arc::new(styles::load(None, None).unwrap_or_default()),
        ..App::default()
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
        .route("/themes/{file}", get(theme))
        .route("/transitions/{file}", get(transition))
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
        .route("/api/sessions/{id}/revisions", get(list_revisions))
        .route("/api/sessions/{id}/revisions/{rev}", get(get_revision))
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
    version: &'static str,
    sessions: usize,
    viewers: usize,
}

/// Counts only. A probe has no business knowing what any room holds.
async fn health(State(registry): State<Registry>) -> Response {
    axum::Json(Health {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
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
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<DeckBody>,
) -> Response {
    if let Some(key) = &app.create_key {
        let offered = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .unwrap_or("");
        let matches: bool = key.as_bytes().ct_eq(offered.trim().as_bytes()).into();
        if !matches {
            return (
                StatusCode::FORBIDDEN,
                "this instance needs a key to start a room",
            )
                .into_response();
        }
    }
    if body.markdown.len() > MAX_DECK_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, "deck too large").into_response();
    }
    if !app
        .limiter
        .take(client_ip(&app, &headers, peer), Limit::Create)
    {
        return (StatusCode::TOO_MANY_REQUESTS, Limit::Create.refused()).into_response();
    }
    let Some((id, token)) = app.registry.create(&body.markdown) else {
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
    headers: HeaderMap,
) -> Response {
    let token = &token_from(&headers, &params);
    let with_people = params
        .get("people")
        .is_some_and(|v| v == "1" || v == "true");
    // Only the copy happens under the lock. Zipping an evening walks every deck
    // and every picture, and the whole instance shares that one lock.
    let view = registry.with(&id, |s| {
        s.role_of(token).hosts().then(|| s.export_view(with_people))
    });
    let built = match view {
        None => None,
        Some(None) => Some(None),
        Some(Some(view)) => Some(Some(
            tokio::task::spawn_blocking(move || crate::export::bundle(&view))
                .await
                .unwrap_or_else(|_| Err(std::io::Error::other("the export panicked"))),
        )),
    };
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
    headers: HeaderMap,
) -> Response {
    let token = &token_from(&headers, &params);
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
    headers: HeaderMap,
    axum::Json(body): axum::Json<TalkEdit>,
) -> Response {
    let token = &token_from(&headers, &params);
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
    /// Every look a deck here may ask for, each with what its own file says it
    /// looks like. The views hold no list of their own: an instance the
    /// operator added to has more than the binary ships.
    themes: Vec<Look>,
    transitions: Vec<Look>,
}

/// What this instance lets a view offer. A page that cannot upload should not
/// show a button that fails.
async fn config(State(app): State<App>) -> Response {
    axum::Json(Config {
        uploads: app.uploads,
        themes: app.styles.themes(),
        transitions: app.styles.transitions(),
    })
    .into_response()
}

async fn theme(State(app): State<App>, Path(file): Path<String>, headers: HeaderMap) -> Response {
    let sheet = name_of(&file).and_then(|name| app.styles.theme(&name));
    stylesheet(sheet, &headers)
}

async fn transition(
    State(app): State<App>,
    Path(file): Path<String>,
    headers: HeaderMap,
) -> Response {
    let sheet = name_of(&file).and_then(|name| app.styles.transition(&name));
    stylesheet(sheet, &headers)
}

/// The name inside `<name>.css`, and only if it is a name. A look is addressed
/// by a name a deck may write, so anything that is not one is not found here
/// rather than being looked up and happening to miss.
fn name_of(file: &str) -> Option<String> {
    crate::deck::style_name(file.strip_suffix(".css")?)
}

/// `no-cache` with an etag over the bytes, like every other asset: a room that
/// already holds a look pays a 304 for it, and an operator who edits a file and
/// restarts gets the new one past every cache.
fn stylesheet(sheet: Option<&crate::styles::Sheet>, request: &HeaderMap) -> Response {
    let Some(sheet) = sheet else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    if request
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|candidate| candidate.trim() == sheet.etag)
        })
    {
        return (
            StatusCode::NOT_MODIFIED,
            [(header::ETAG, sheet.etag.clone())],
        )
            .into_response();
    }

    (
        [
            (header::CONTENT_TYPE, "text/css".to_string()),
            (header::CACHE_CONTROL, "no-cache".to_string()),
            (header::ETAG, sheet.etag.clone()),
        ],
        sheet.css.clone(),
    )
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

    let token = &token_from(&headers, &params);
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

    // Refused rather than queued: a phone waiting on a picture behind a queue
    // of other people's pictures is a phone that looks broken.
    let Ok(_decoding) = app.decoding.clone().try_acquire_owned() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "the room is busy with another picture, try again",
        )
            .into_response();
    };

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
async fn preview_deck(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<DeckBody>,
) -> Response {
    if body.markdown.len() > MAX_DECK_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, "deck too large").into_response();
    }
    if !app
        .limiter
        .take(client_ip(&app, &headers, peer), Limit::Pack)
    {
        return (StatusCode::TOO_MANY_REQUESTS, Limit::Pack.refused()).into_response();
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
async fn pack_deck(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<DeckBody>,
) -> Response {
    if !app
        .limiter
        .take(client_ip(&app, &headers, peer), Limit::Pack)
    {
        return (StatusCode::TOO_MANY_REQUESTS, Limit::Pack.refused()).into_response();
    }
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
    headers: HeaderMap,
    axum::Json(body): axum::Json<DeckBody>,
) -> Response {
    if body.markdown.len() > MAX_DECK_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, "deck too large").into_response();
    }
    let token = &token_from(&headers, &params);
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
    headers: HeaderMap,
) -> Response {
    let token = &token_from(&headers, &params);
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
    headers: HeaderMap,
) -> Response {
    let token = &token_from(&headers, &params);
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
    headers: HeaderMap,
) -> Response {
    let token = &token_from(&headers, &params);
    if !registry.role(&id, token).edits() {
        return StatusCode::FORBIDDEN.into_response();
    }
    match registry.markdown(&id) {
        Some(markdown) => markdown.into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// The decks earlier saves replaced, newest first. For whoever may edit, which
/// is who would want one back.
async fn list_revisions(
    State(registry): State<Registry>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let token = &token_from(&headers, &params);
    let found = registry.with(&id, |s| s.role_of(token).edits().then(|| s.revisions()));
    match found {
        Some(Some(revisions)) => axum::Json(revisions).into_response(),
        Some(None) => StatusCode::FORBIDDEN.into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn get_revision(
    State(registry): State<Registry>,
    Path((id, rev)): Path<(String, u64)>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let token = &token_from(&headers, &params);
    let found = registry.with(&id, |s| {
        s.role_of(token)
            .edits()
            .then(|| s.revision(rev).map(str::to_owned))
    });
    match found {
        Some(Some(Some(markdown))) => (
            [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
            markdown,
        )
            .into_response(),
        Some(Some(None)) | None => StatusCode::NOT_FOUND.into_response(),
        Some(None) => StatusCode::FORBIDDEN.into_response(),
    }
}

async fn socket(
    State(app): State<App>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let registry = app.registry.clone();
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
        .on_upgrade(move |sock| ws::serve(sock, registry, join, app.heartbeat))
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
