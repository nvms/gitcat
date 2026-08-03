use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use gitcat::{Config, web};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(version, about = "A self-hosted git server", long_about = None)]
struct Cli {
    /// Directory containing bare repositories
    #[arg(short, long, env = "GITCAT_REPOS", default_value = ".")]
    repos: PathBuf,

    /// Address to bind the HTTP server to
    #[arg(short, long, env = "GITCAT_BIND", default_value = "127.0.0.1:9090")]
    bind: SocketAddr,

    /// Name shown in the page title and index heading
    #[arg(long, env = "GITCAT_SITE_NAME", default_value = "gitcat")]
    site_name: String,

    /// Origin used to build clone URLs, e.g. https://git.example.com. Defaults
    /// to the Host header of each request.
    #[arg(long, env = "GITCAT_BASE_URL")]
    base_url: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("GITCAT_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Arc::new(Config::new(&cli.repos, cli.site_name)?.with_base_url(cli.base_url));
    let listener = tokio::net::TcpListener::bind(cli.bind)
        .await
        .with_context(|| format!("failed to bind {}", cli.bind))?;

    tracing::info!(
        address = %listener.local_addr().unwrap_or(cli.bind),
        repos = %config.repos.display(),
        "gitcat listening"
    );

    axum::serve(listener, web::router(config))
        .with_graceful_shutdown(shutdown())
        .await
        .context("server error")
}

async fn shutdown() {
    if let Err(e) = tokio::signal::ctrl_c().await {
        tracing::error!(error = %e, "failed to listen for shutdown signal");
    }
}
