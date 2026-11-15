mod client;
pub mod config;
mod prompt;
mod response;

pub use config::AiConfig;

use std::time::Duration;

use anyhow::{Context, Result, bail};
use rand::Rng;
use serde_json::json;
use sqlx::PgPool;
use tokio::time::sleep;

use crate::features::juego::{self, JuegoClient};
use crate::infrastructure::postgres;
use crate::shared::text::{now_iso, truncate};

pub async fn run_loop(
    client: &reqwest::Client,
    config: &AiConfig,
    pool: &PgPool,
    dry_run: bool,
    juego_client: Option<&JuegoClient>,
) -> Result<()> {
    config.ensure_ready()?;

    loop {
        let wait_minutes = rand::rng().random_range(config.min_minutes..=config.max_minutes);
        eprintln!(
            "[{}] IA: próximo evento en {} minuto(s)...",
            now_iso(),
            wait_minutes
        );

        tokio::select! {
            _ = sleep(Duration::from_secs(wait_minutes.saturating_mul(60))) => {}
            signal = tokio::signal::ctrl_c() => {
                signal.context("No se pudo escuchar Ctrl+C")?;
                eprintln!("[{}] IA: detenida por el usuario.", now_iso());
                break;
            }
        }

        match dispatch_once(client, config, pool, dry_run, juego_client).await {
            Ok(true) => {}
            Ok(false) => {
                eprintln!(
                    "[{}] IA: no hay eventos sin mensaje; se espera al siguiente ciclo.",
                    now_iso()
                );
            }
            Err(error) => {
                eprintln!("[{}] IA: error: {error:#}", now_iso());
            }
        }
    }

    Ok(())
}

pub async fn dispatch_once(
    client: &reqwest::Client,
    config: &AiConfig,
    pool: &PgPool,
    dry_run: bool,
    juego_client: Option<&JuegoClient>,
) -> Result<bool> {
    let api_key = config.api_key()?;

    let ids = postgres::mensajes::pending_ids(pool, config.pending_limit).await?;

    if ids.is_empty() {
        eprintln!(
            "[{}] IA: no hay eventos sin mensaje; no se envía nada al proveedor de IA.",
            now_iso()
        );
        return Ok(false);
    }

    let event_id = ids[rand::rng().random_range(0..ids.len())];
    let Some(event) = postgres::mensajes::fetch_by_id(pool, event_id).await? else {
        eprintln!(
            "[{}] IA: el evento {event_id} desapareció; se espera al siguiente ciclo.",
            now_iso()
        );
        return Ok(false);
    };

    eprintln!(
        "[{}] IA: evento elegido id={} tipo={} donki_id={}",
        now_iso(),
        event.id,
        event.tipo,
        event.donki_id.as_deref().unwrap_or("-")
    );

    let content = client::call(client, config, api_key, &event).await?;
    let contenido = response::parse(&content);

    if contenido.mensaje.trim().is_empty() {
        bail!("El proveedor de IA devolvió un mensaje vacío");
    }

    if dry_run {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "evento_id": event.id,
                "donki_id": event.donki_id,
                "tipo": event.tipo,
                "mensaje": &contenido.mensaje,
                "explicacion": &contenido.explicacion,
            }))?
        );
        eprintln!(
            "[{}] IA: resultado mostrado en dry-run, no se guardó.",
            now_iso()
        );

        if let Some(juego) = juego_client {
            let texto_pantalla = truncate(&contenido.mensaje, juego::MAX_MENSAJE_CHARS);
            let msg_pantalla = juego::construir_mensaje_pantalla(
                1,
                &texto_pantalla,
                &juego.config.color,
                juego.config.segundos,
            );
            println!("{}", serde_json::to_string_pretty(&msg_pantalla)?);
            eprintln!(
                "[{}] Juego: modo dry-run, mensaje de prueba no enviado al relay.",
                now_iso()
            );
        }

        return Ok(true);
    }

    if postgres::mensajes::insert(pool, event.id, &contenido).await? {
        eprintln!(
            "[{}] IA: mensaje guardado para evento {} ({}).",
            now_iso(),
            event.id,
            event.tipo
        );
    } else {
        eprintln!(
            "[{}] IA: otra instancia ya guardó el mensaje del evento {} ({}).",
            now_iso(),
            event.id,
            event.tipo
        );
    }

    // Enviar alerta al juego sin tumbar el proceso ni alterar el guardado en base
    if let Some(juego) = juego_client {
        let texto_pantalla = truncate(&contenido.mensaje, juego::MAX_MENSAJE_CHARS);
        if let Err(error) = juego
            .enviar_mensaje(&texto_pantalla, &juego.config.color, juego.config.segundos)
            .await
        {
            eprintln!(
                "[{}] Juego: no se pudo enviar el mensaje a la pantalla: {error:#}",
                now_iso()
            );
        }
    }

    Ok(true)
}
