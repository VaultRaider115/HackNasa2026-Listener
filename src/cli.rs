use std::env;

use anyhow::{Result, bail};

use crate::config::AppConfig;
use crate::features::ia::AiConfig;
use crate::features::nasa::ENDPOINTS;

#[derive(Default)]
pub struct Args {
    pub once: bool,
    pub dry_run: bool,
    pub ai_once: bool,
    pub ai_only: bool,
}

pub fn parse_args() -> Result<Args> {
    let mut args = Args::default();

    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--once" => args.once = true,
            "--dry-run" => args.dry_run = true,
            "--ai-once" => args.ai_once = true,
            "--ai-only" => args.ai_only = true,
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            other => bail!("Argumento no reconocido: {other}. Usa --help para ver las opciones."),
        }
    }

    if args.ai_only && args.once {
        bail!("--ai-only no se puede combinar con --once");
    }

    if args.ai_once && args.ai_only {
        bail!("--ai-once no se puede combinar con --ai-only");
    }

    Ok(args)
}

pub fn print_help() {
    println!(
        "Listener NASA DONKI + mensajes IA\n\
         \n\
         Uso: listener_nasa_game [--once] [--dry-run] [--ai-once] [--ai-only]\n\
         \n\
         Opciones:\n\
           --once      Ejecuta un solo ciclo de NASA y termina.\n\
           --dry-run   No escribe en la base de datos; con --ai-once muestra el resultado IA.\n\
           --ai-once   Envía un evento pendiente a la IA una vez y termina.\n\
           --ai-only   Ejecuta solo el worker de IA (sin consultar NASA).\n\
           -h, --help  Muestra esta ayuda.\n\
         \n\
         Variables de entorno principales:\n\
           API_NASA_KEY (o NASA_API_KEY)  API key de NASA (solo listener NASA).\n\
           DATABASE_URL                   Cadena de conexión Postgres.\n\
           DIAS_VENTANA                   Días a consultar (2 = ayer y hoy).\n\
           INTERVALO_MINUTOS              Minutos entre ciclos de NASA.\n\
           DRY_RUN                        true para no escribir en la base de datos.\n\
           AI_ENABLED                     true/false para activar el worker IA.\n\
           AI_API_KEY (o API_IA_KEY)      API key del proveedor de IA.\n\
           AI_API_URL                     URL completa del endpoint de chat.\n\
           AI_MODEL                       Modelo del proveedor de IA.\n\
           AI_MIN_MINUTOS / AI_MAX_MINUTOS Rango aleatorio entre envíos (15/60).\n\
           AI_PROMPT                      Prompt personalizado (opcional).\n"
    );
}

pub fn print_banner(config: &AppConfig, ai_config: &AiConfig) {
    eprintln!("Listener NASA DONKI + mensajes IA");
    eprintln!(
        "  Ventana: {} día(s) (2 = ayer y hoy, UTC)",
        config.dias_ventana
    );
    eprintln!("  Intervalo NASA: {} minuto(s)", config.intervalo_minutos);
    eprintln!("  Endpoints: {}", ENDPOINTS.join(", "));

    if ai_config.enabled() {
        eprintln!(
            "  IA: {} cada {}-{} minuto(s)",
            ai_config.model, ai_config.min_minutes, ai_config.max_minutes
        );
    } else {
        eprintln!(
            "  IA: deshabilitada ({})",
            ai_config
                .disabled_reason
                .as_deref()
                .unwrap_or("sin configurar")
        );
    }

    if config.dry_run {
        eprintln!("  Modo: DRY RUN (sin escribir en la base de datos)");
    } else {
        eprintln!("  Postgres: configurado");
    }
}
