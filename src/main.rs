use anyhow::Result;
use clap::Parser;
use tgb_cli::{app, cli::Cli};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("tgb=info,tgb_cli=info")),
        )
        .with_target(false)
        .compact()
        .init();

    app::run(Cli::parse()).await
}
