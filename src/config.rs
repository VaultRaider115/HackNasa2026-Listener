use anyhow::{Context, Result, bail};

use crate::shared::env::{env_bool_or, env_parse, first_non_empty};

pub const NASA_BASE_DEFAULT: &str = "https://api.nasa.gov";
pub const HTTP_TIMEOUT_SECS: u64 = 45;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub nasa_api_key: Option<String>,
    pub nasa_base_url: String,
    pub database_url: Option<String>,
    pub dias_ventana: i64,
    pub intervalo_minutos: u64,
    pub dry_run: bool,
}

impl AppConfig {
    pub fn from_env(cli_dry_run: bool) -> Result<Self> {
        let nasa_api_key = first_non_empty(&["API_NASA_KEY", "NASA_API_KEY"]);

        let dry_run = cli_dry_run || env_bool_or("DRY_RUN", false);
        let database_url = first_non_empty(&["DATABASE_URL"]);

        if !dry_run && database_url.is_none() {
            bail!(
                "Falta DATABASE_URL. Añádela a .env con la cadena de conexión Postgres de Supabase."
            );
        }

        let dias_ventana = env_parse::<i64>("DIAS_VENTANA", 2)?;
        if dias_ventana < 1 {
            bail!("DIAS_VENTANA debe ser mayor o igual a 1");
        }

        let intervalo_minutos = env_parse::<u64>("INTERVALO_MINUTOS", 60)?;
        if intervalo_minutos < 1 {
            bail!("INTERVALO_MINUTOS debe ser mayor o igual a 1");
        }

        Ok(Self {
            nasa_api_key,
            nasa_base_url: first_non_empty(&["NASA_API_BASE"])
                .unwrap_or_else(|| NASA_BASE_DEFAULT.to_string()),
            database_url,
            dias_ventana,
            intervalo_minutos,
            dry_run,
        })
    }

    pub fn nasa_api_key(&self) -> Result<&str> {
        self.nasa_api_key
            .as_deref()
            .context("Falta API_NASA_KEY (o NASA_API_KEY) para consultar NASA")
    }
}
