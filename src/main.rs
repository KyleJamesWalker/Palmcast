use std::time::Duration;

use std::path::PathBuf;

use clap::Parser;
use palmcast::persist;
use palmcast::routes::{self, App};
use palmcast::session::Registry;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "palmcast", about = "Live slides on every screen in the room")]
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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let args = Args::parse();
    let registry = Registry::new(Duration::from_secs(args.ttl_hours * 3600));

    if let Some(path) = &args.state_file {
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
                let saved = saver.export();
                if let Err(error) = persist::save(&path, &saved) {
                    tracing::error!(%error, "periodic save failed");
                }
            }
        });
    }

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
    let app = App {
        registry: registry.clone(),
        public_url: args.public_url.clone(),
    };
    // The result is held rather than propagated, because a server that fell over
    // still has rooms worth keeping and `?` here would skip the save entirely.
    let outcome = axum::serve(listener, routes::router_with(app))
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
