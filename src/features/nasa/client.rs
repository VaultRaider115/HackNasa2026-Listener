use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use chrono::NaiveDate;
use serde_json::Value;
use tokio::time::sleep;

use crate::config::AppConfig;
use crate::shared::text::truncate;

const MAX_RETRIES: u32 = 3;
// NASA movió FLR a CCMC; la ruta antigua de api.nasa.gov devuelve 404.
const FLR_URL: &str = "https://ccmc.gsfc.nasa.gov/DONKI-API/get/FLR";

pub async fn fetch_donki(
    client: &reqwest::Client,
    config: &AppConfig,
    endpoint: &str,
    since: NaiveDate,
    until: NaiveDate,
) -> Result<Vec<Value>> {
    let url = if endpoint == "FLR" {
        FLR_URL.to_string()
    } else {
        format!(
            "{}/DONKI/{}",
            config.nasa_base_url.trim_end_matches('/'),
            endpoint
        )
    };
    let api_key = if endpoint == "FLR" {
        None
    } else {
        Some(config.nasa_api_key()?)
    };
    let start_date = since.format("%Y-%m-%d").to_string();
    let end_date = until.format("%Y-%m-%d").to_string();
    let mut last_error: Option<anyhow::Error> = None;

    for attempt in 1..=MAX_RETRIES {
        let mut request = client.get(&url).query(&[
            ("startDate", start_date.as_str()),
            ("endDate", end_date.as_str()),
        ]);

        if let Some(api_key) = api_key {
            request = request.query(&[("api_key", api_key)]);
        }

        let response = request.send().await;

        match response {
            Ok(response) if response.status().is_success() => {
                let body = response
                    .text()
                    .await
                    .context("No se pudo leer la respuesta de NASA")?;
                let parsed: Value = serde_json::from_str(&body).with_context(|| {
                    format!("Respuesta no JSON de {endpoint}: {}", truncate(&body, 200))
                })?;

                return match parsed {
                    Value::Array(items) => Ok(items),
                    Value::Null => Ok(Vec::new()),
                    other => Err(anyhow!(
                        "Respuesta inesperada de {endpoint}: {}",
                        truncate(&other.to_string(), 300)
                    )),
                };
            }
            Ok(response) => {
                let status = response.status();
                let retryable = status.as_u16() == 429 || status.is_server_error();
                let body = response.text().await.unwrap_or_default();
                let message = format!("HTTP {status} de {endpoint}: {}", truncate(&body, 300));

                if retryable && attempt < MAX_RETRIES {
                    sleep(backoff(attempt)).await;
                    last_error = Some(anyhow!(message));
                    continue;
                }

                return Err(anyhow!(message));
            }
            Err(error) => {
                let retryable = error.is_timeout() || error.is_connect() || error.is_request();
                let message = format!(
                    "Error de red consultando {endpoint}: {}",
                    error.without_url()
                );

                if retryable && attempt < MAX_RETRIES {
                    sleep(backoff(attempt)).await;
                    last_error = Some(anyhow!(message));
                    continue;
                }

                return Err(anyhow!(message));
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("Error desconocido consultando {endpoint}")))
}

fn backoff(attempt: u32) -> Duration {
    Duration::from_secs(2u64.pow(attempt.saturating_sub(1)))
}
