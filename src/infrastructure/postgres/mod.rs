pub mod eventos;
pub mod mensajes;
pub mod sync;

use anyhow::{Context, Result};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

pub fn connect(database_url: &str) -> Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(5)
        .connect_lazy(database_url)
        .context("DATABASE_URL inválida")
}
