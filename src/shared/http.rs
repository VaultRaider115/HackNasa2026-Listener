use std::time::Duration;

use anyhow::{Context, Result};

pub fn build_client(timeout: Duration) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(format!("listener-nasa-game/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("No se pudo crear el cliente HTTP")
}
