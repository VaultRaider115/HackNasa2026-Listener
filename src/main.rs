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
    let config = config::AppConfig::from_env(args.dry_run, args.juego_test.is_some())?;
    let ai_config = features::ia::AiConfig::from_env()?;
    let juego_config = features::juego::JuegoConfig::from_env()?;

    let client = shared::http::build_client(Duration::from_secs(config::HTTP_TIMEOUT_SECS))?;
    cli::print_banner(
        &config,
        &ai_config,
        &juego_config,
        args.juego_test.is_some(),
    );

    // --juego-test no debe depender de Postgres, aunque DATABASE_URL esté definida.
    let pool = if args.juego_test.is_some() {
        None
    } else {
        match config.database_url.as_deref() {
            Some(url) => Some(infrastructure::postgres::connect(url)?),
            None => None,
        }
    };

    app::run(
        &client,
        pool.as_ref(),
        &config,
        &ai_config,
        &juego_config,
        &args,
    )
    .await
}
