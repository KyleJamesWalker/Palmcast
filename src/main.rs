use std::time::Duration;

use clap::Parser;
use palmcast::routes;
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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let args = Args::parse();
    let registry = Registry::new(Duration::from_secs(args.ttl_hours * 3600));

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
    axum::serve(listener, routes::router(registry)).await?;
    Ok(())
}
