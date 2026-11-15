mod app;
mod cli;
mod config;
mod domain;
mod features;
mod infrastructure;
mod shared;

use std::time::Duration;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    shared::env::load_env_files();

    let args = cli::parse_args()?;
    let config = config::AppConfig::from_env(args.dry_run)?;
    let ai_config = features::ia::AiConfig::from_env()?;

    let client = shared::http::build_client(Duration::from_secs(config::HTTP_TIMEOUT_SECS))?;
    cli::print_banner(&config, &ai_config);

    let pool = match config.database_url.as_deref() {
        Some(url) => Some(infrastructure::postgres::connect(url)?),
        None => None,
    };

    app::run(&client, pool.as_ref(), &config, &ai_config, &args).await
}
