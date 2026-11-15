use std::time::Duration;

use anyhow::{Context, Result};
use sqlx::PgPool;
use tokio::time::sleep;

use crate::cli::Args;
use crate::config::AppConfig;
use crate::features::ia::{self, AiConfig};
use crate::features::nasa;
use crate::shared::text::now_iso;

pub async fn run(
    client: &reqwest::Client,
    pool: Option<&PgPool>,
    config: &AppConfig,
    ai_config: &AiConfig,
    args: &Args,
) -> Result<()> {
    if args.ai_once {
        ai_config.ensure_ready()?;
        let pool = pool.context("Falta DATABASE_URL para leer eventos")?;
        ia::dispatch_once(client, ai_config, pool, config.dry_run).await?;

        if !args.once {
            return Ok(());
        }
    }

    if args.ai_only {
        ai_config.ensure_ready()?;
        let pool = pool.context("Falta DATABASE_URL para leer eventos")?;
        ia::run_loop(client, ai_config, pool, config.dry_run).await?;
        return Ok(());
    }

    let mut ai_worker = None;

    if !args.once && !config.dry_run && ai_config.enabled() {
        match pool.cloned() {
            Some(ai_pool) => {
                let ai_client = client.clone();
                let ai_config = ai_config.clone();

                ai_worker = Some(tokio::spawn(async move {
                    if let Err(error) = ia::run_loop(&ai_client, &ai_config, &ai_pool, false).await
                    {
                        eprintln!("[{}] IA: worker detenido: {error:#}", now_iso());
                    }
                }));
            }
            None => eprintln!("[{}] IA: falta DATABASE_URL; worker no iniciado", now_iso()),
        }
    }

    loop {
        let result = nasa::run_cycle(client, pool, config).await;

        if let Err(error) = &result {
            eprintln!("[{}] Ciclo con error: {error:#}", now_iso());
        }

        if args.once {
            result?;
            break;
        }

        eprintln!(
            "[{}] Próximo ciclo en {} minuto(s). Ctrl+C para detener.",
            now_iso(),
            config.intervalo_minutos
        );

        tokio::select! {
            _ = sleep(Duration::from_secs(config.intervalo_minutos.saturating_mul(60))) => {}
            signal = tokio::signal::ctrl_c() => {
                signal.context("No se pudo escuchar Ctrl+C")?;
                eprintln!("[{}] Detenido por el usuario.", now_iso());
                break;
            }
        }
    }

    if let Some(worker) = ai_worker {
        worker.abort();
    }

    Ok(())
}
