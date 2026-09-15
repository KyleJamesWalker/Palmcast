use std::sync::Arc;
use std::time::Duration;

use std::path::{Path, PathBuf};

use clap::Parser;
use palmcast::persist;
use palmcast::routes::{self, App};
use palmcast::session::{self, Registry};
use palmcast::styles;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "palmcast",
    version,
    about = "Live slides on every screen in the room"
)]
struct Args {
    #[arg(long, env = "PALMCAST_PORT", default_value_t = 8080)]
    port: u16,

    #[arg(long, env = "PALMCAST_BIND", default_value = "0.0.0.0")]
    bind: String,

    /// Hours a session survives with nobody watching it.
    #[arg(long, env = "PALMCAST_TTL_HOURS", default_value_t = 6)]
    ttl_hours: u64,

    /// The address the audience reaches, such as https://palmcast.example.
    /// Without it the QR code trusts the Host header, which is right on a
    /// laptop and a guess behind a proxy.
    #[arg(long, env = "PALMCAST_PUBLIC_URL")]
    public_url: Option<String>,

    /// Carry live rooms across a restart. Holds presenter tokens, so the file
    /// is written 0600. Leave unset to keep everything in memory.
    #[arg(long, env = "PALMCAST_STATE_FILE")]
    state_file: Option<PathBuf>,

    /// A Markdown deck the start page opens with, instead of the built-in
    /// sample. For an instance that runs the same quiz or talk every time.
    #[arg(long, env = "PALMCAST_DECK")]
    deck: Option<PathBuf>,

    /// Themes to serve on top of the built-in ones, as a directory of css
    /// files. The file name is the name a deck asks for, so `dusk.css` is
    /// `<!-- theme: dusk -->`. A file named after a built-in replaces it.
    #[arg(long, env = "PALMCAST_THEME_DIR")]
    theme_dir: Option<PathBuf>,

    /// Transitions to serve on top of the built-in ones, on the same terms as
    /// `--theme-dir`. `web/transitions` in the source tree is the worked
    /// example: every built-in is an ordinary file in that shape.
    #[arg(long, env = "PALMCAST_TRANSITION_DIR")]
    transition_dir: Option<PathBuf>,

    /// Keep pictures people upload, for as long as the room that holds them.
    /// Off by default: it is the one thing here that holds bytes a stranger
    /// chose, and a public instance should say yes on purpose.
    #[arg(long, env = "PALMCAST_UPLOADS", default_value_t = false)]
    uploads: bool,

    /// Require this key to start a room. The start page reads it from a `#k=`
    /// fragment, so an operator hands out one link and the key never reaches a
    /// server log. Unset leaves the instance open to anyone who can reach it.
    #[arg(long, env = "PALMCAST_CREATE_KEY")]
    create_key: Option<String>,

    /// Rooms to hold at once. Each holds a deck, its votes and any pictures.
    #[arg(long, env = "PALMCAST_MAX_SESSIONS", default_value_t = session::DEFAULT_MAX_SESSIONS)]
    max_sessions: usize,

    /// Rooms one address may start in an hour.
    #[arg(long, env = "PALMCAST_CREATE_PER_HOUR", default_value_t = 10)]
    create_per_hour: u32,

    /// Decks one address may pack or preview in a minute.
    #[arg(long, env = "PALMCAST_PACK_PER_MINUTE", default_value_t = 60)]
    pack_per_minute: u32,
}

/// Read once at startup, not per request: a deck the operator named and the
/// server cannot use is a mistake worth stopping for, and the first visitor of
/// the evening is too late to find it.
fn read_deck(path: &Path) -> Result<String, String> {
    let markdown = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read the deck at {}: {error}", path.display()))?;
    if markdown.len() > routes::MAX_DECK_BYTES {
        return Err(format!(
            "the deck at {} is {} bytes, over the {} byte limit",
            path.display(),
            markdown.len(),
            routes::MAX_DECK_BYTES
        ));
    }
    Ok(markdown)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let args = Args::parse();
    let registry = Registry::with_limit(
        Duration::from_secs(args.ttl_hours * 3600),
        args.max_sessions,
    );

    // Same reasoning as the starter deck below: a directory the operator named
    // and this cannot read is a mistake worth stopping for, because the first
    // deck of the evening is too late to find out its theme never loaded.
    let styles = Arc::new(styles::load(
        args.theme_dir.as_deref(),
        args.transition_dir.as_deref(),
    )?);
    tracing::info!(
        themes = styles.theme_names().len(),
        transitions = styles.transition_names().len(),
        "looks a deck may name"
    );

    let starter = match &args.deck {
        Some(path) => {
            let markdown = read_deck(path)?;
            tracing::info!(
                bytes = markdown.len(),
                "start page opens with {}",
                path.display()
            );
            Some(markdown)
        }
        None => None,
    };

    if let Some(path) = &args.state_file {
        persist::warn_if_unprotected(path);
        match persist::load(path) {
            Ok(saved) if saved.is_empty() => {}
            Ok(saved) => {
                let restored = registry.import(saved);
                // Whatever ran out while the process was down goes now, rather
                // than occupying the instance until the first sweep is due.
                let expired = registry.sweep();
                tracing::info!(
                    restored,
                    expired,
                    live = registry.len(),
                    "restored rooms from {}",
                    path.display()
                );
            }
            // A bad state file must not stop the server: an empty instance
            // still works, a dead one does not.
            Err(error) => tracing::error!(%error, "could not read {}", path.display()),
        }
    }

    if let Some(path) = args.state_file.clone() {
        let saver = registry.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(60));
            loop {
                tick.tick().await;
                // An instance nobody is using rewrote the same file every
                // minute, cloning every room under the lock to do it.
                if !saver.changed() {
                    continue;
                }
                let saved = saver.export();
                if let Err(error) = persist::save(&path, &saved) {
                    tracing::error!(%error, "periodic save failed");
                }
            }
        });
    }

    // The room's size is told on a timer rather than on every arrival, so a
    // QR code going up is one frame and not one per phone.
    let counter = registry.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            counter.flush_viewers();
        }
    });

    let sweeper = registry.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(300));
        loop {
            tick.tick().await;
            let dropped = sweeper.sweep();
            if dropped > 0 {
                tracing::info!(dropped, live = sweeper.len(), "swept idle sessions");
            }
        }
    });

    let listener = tokio::net::TcpListener::bind((args.bind.as_str(), args.port)).await?;
    tracing::info!("palmcast listening on http://{}", listener.local_addr()?);
    if args.uploads {
        tracing::info!(
            max_edge = palmcast::images::MAX_EDGE,
            max_bytes = palmcast::images::MAX_UPLOAD_BYTES,
            "keeping uploaded images for the life of a room"
        );
    }

    let app = App {
        registry: registry.clone(),
        public_url: args.public_url.clone(),
        starter,
        uploads: args.uploads,
        styles,
        create_key: args.create_key.clone(),
        limiter: Arc::new(routes::Limiter::new(
            args.create_per_hour,
            args.pack_per_minute,
        )),
        ..App::default()
    };
    // The result is held rather than propagated, because a server that fell over
    // still has rooms worth keeping and `?` here would skip the save entirely.
    // The connect info is what the rate limiter meters on.
    let outcome = axum::serve(
        listener,
        routes::router_with(app).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await;

    if let Some(path) = &args.state_file {
        let saved = registry.export();
        match persist::save(path, &saved) {
            Ok(()) => tracing::info!(rooms = saved.len(), "saved to {}", path.display()),
            Err(error) => tracing::error!(%error, "could not save {}", path.display()),
        }
    }
    outcome?;
    Ok(())
}

/// Docker stops a container with SIGTERM, so without this every live room dies
/// mid-slide on an ordinary redeploy.
async fn shutdown() {
    let interrupt = async {
        tokio::signal::ctrl_c().await.ok();
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                term.recv().await;
            }
            Err(error) => tracing::warn!(%error, "no SIGTERM handler"),
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = interrupt => {}
        _ = terminate => {}
    }
    tracing::info!("shutting down");
}
