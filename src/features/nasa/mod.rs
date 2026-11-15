pub mod client;
pub mod mapping;

use anyhow::{Context, Result, bail};
use chrono::{NaiveDate, TimeDelta, Utc};
use sqlx::PgPool;

use crate::config::AppConfig;
use crate::infrastructure::postgres;
use crate::shared::text::{now_iso, redact};

pub const ENDPOINTS: [&str; 5] = ["CME", "GST", "SEP", "FLR", "IPS"];

enum EndpointError {
    Nasa(anyhow::Error),
    Write(anyhow::Error),
}

pub async fn run_cycle(
    client: &reqwest::Client,
    pool: Option<&PgPool>,
    config: &AppConfig,
) -> Result<usize> {
    config.nasa_api_key()?;

    let db = if config.dry_run {
        None
    } else {
        Some(pool.context("Falta DATABASE_URL para escribir en Postgres")?)
    };
    let today = Utc::now().date_naive();
    let since = today - TimeDelta::days(config.dias_ventana - 1);

    eprintln!(
        "\n[{}] Consultando NASA DONKI del {since} al {today} (UTC)",
        now_iso()
    );

    let mut nasa_ok = 0usize;
    let mut write_ok = 0usize;

    for endpoint in ENDPOINTS {
        match process_endpoint(client, db, config, endpoint, since, today).await {
            Ok(()) => {
                nasa_ok += 1;
                write_ok += 1;
            }
            Err(EndpointError::Nasa(error)) => {
                eprintln!("[{}] {endpoint}: error: {error:#}", now_iso());
            }
            Err(EndpointError::Write(error)) => {
                nasa_ok += 1;
                eprintln!("[{}] {endpoint}: error de escritura: {error:#}", now_iso());
            }
        }
    }

    if nasa_ok == 0 {
        bail!("Todos los endpoints de NASA fallaron");
    }

    if write_ok == 0 {
        bail!("NASA respondió, pero no se pudo escribir en Postgres");
    }

    Ok(write_ok)
}

async fn process_endpoint(
    client: &reqwest::Client,
    db: Option<&PgPool>,
    config: &AppConfig,
    endpoint: &str,
    since: NaiveDate,
    until: NaiveDate,
) -> std::result::Result<(), EndpointError> {
    let items = match client::fetch_donki(client, config, endpoint, since, until).await {
        Ok(items) => items,
        Err(error) => {
            let message = redact(&format!("{error:#}"), config.nasa_api_key().unwrap_or(""));
            record_sync_error(db, config, endpoint, &message).await;
            return Err(EndpointError::Nasa(anyhow::anyhow!("{message}")));
        }
    };

    let received = items.len();
    let mut rows = Vec::with_capacity(received);

    for item in &items {
        match mapping::map_event(endpoint, item) {
            Ok(row) => rows.push(row),
            Err(error) => {
                eprintln!("[{}] {endpoint}: evento omitido: {error:#}", now_iso());
            }
        }
    }

    let new_count = if config.dry_run {
        for row in &rows {
            println!(
                "{}",
                serde_json::to_string(row).map_err(|error| EndpointError::Write(error.into()))?
            );
        }
        rows.len()
    } else {
        let pool = db
            .context("Falta DATABASE_URL para escribir en Postgres")
            .map_err(EndpointError::Write)?;

        match postgres::eventos::insert(pool, &rows).await {
            Ok(count) => count,
            Err(error) => {
                let message = redact(&format!("{error:#}"), config.nasa_api_key().unwrap_or(""));
                record_sync_error(db, config, endpoint, &message).await;
                return Err(EndpointError::Write(error));
            }
        }
    };

    if !config.dry_run {
        let pool = db
            .context("Falta DATABASE_URL para actualizar sync_nasa")
            .map_err(EndpointError::Write)?;

        postgres::sync::record(pool, endpoint, new_count as i32, None)
            .await
            .map_err(EndpointError::Write)?;
    }

    println!(
        "[{}] {endpoint}: {received} recibido(s), {new_count} nuevo(s){}",
        now_iso(),
        if config.dry_run { " [dry-run]" } else { "" }
    );

    Ok(())
}

async fn record_sync_error(db: Option<&PgPool>, config: &AppConfig, endpoint: &str, message: &str) {
    if config.dry_run {
        return;
    }

    let Some(pool) = db else {
        eprintln!(
            "[{}] {endpoint}: falta DATABASE_URL para guardar el error",
            now_iso()
        );
        return;
    };

    if let Err(error) = postgres::sync::record(pool, endpoint, 0, Some(message)).await {
        eprintln!(
            "[{}] {endpoint}: no se pudo guardar el error en sync_nasa: {error:#}",
            now_iso()
        );
    }
}
