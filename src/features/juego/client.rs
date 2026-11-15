use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::{Instant, sleep, timeout};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

use crate::shared::text::{now_iso, redact};

use super::config::JuegoConfig;

pub const MAX_MENSAJE_CHARS: usize = 160;
const MAX_CONEXION_REINTENTOS: usize = 4;
const TIMEOUT_CONEXION_SECS: u64 = 60;
const TIMEOUT_HANDSHAKE_SECS: u64 = 10;
const TIMEOUT_RESPUESTA_SECS: u64 = 15;
const MAX_INTENTOS_ENVIO: usize = 2;

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

// Protocolo observado en el .pck publicado del juego (scripts/conexion_mcp.gdc):
// el juego espera {"id", "cmd", "args"} y responde {"id", "ok", "datos", "error", "mensaje"}.
// TODO(protocolo): el comando de pantalla se infiere como "mensaje"; confirmar el nombre
// exacto con el fuente privado mcp/servidor_mcp.py si el juego responde "Comando desconocido".
pub fn construir_mensaje_pantalla(id: u64, texto: &str, color: &str, segundos: u32) -> Value {
    serde_json::json!({
        "id": id,
        "cmd": "mensaje",
        "args": {
            "texto": texto,
            "color": color,
            "segundos": segundos,
        }
    })
}

// El relay de Mission Trainer publica al MCP en /mcp?sala=CODIGO (una sala) y en
// /mcp (global), que reenvía a todas las salas. La sala va en la query, no en un
// mensaje de handshake.
pub fn construir_url_relay(relay_url: &str, sala: Option<&str>) -> String {
    let base = relay_url.trim_end_matches('/');
    match sala {
        Some(sala) => format!("{base}/mcp?sala={sala}"),
        None => format!("{base}/mcp"),
    }
}

enum EnvioError {
    Conexion(anyhow::Error),
    Aplicacion(anyhow::Error),
}

pub struct JuegoClient {
    pub config: JuegoConfig,
    next_id: AtomicU64,
    stream: Mutex<Option<WsStream>>,
}

impl JuegoClient {
    pub fn new(config: JuegoConfig) -> Self {
        Self {
            config,
            next_id: AtomicU64::new(1),
            stream: Mutex::new(None),
        }
    }

    pub fn construir_url_relay(&self) -> String {
        construir_url_relay(&self.config.relay_url, self.config.sala())
    }

    fn destino(&self) -> &'static str {
        // La sala funciona como contraseña; nunca se imprime en los logs.
        if self.config.sala().is_some() {
            "la sala configurada"
        } else {
            "todas las salas (MCP global)"
        }
    }

    pub fn construir_mensaje_pantalla(id: u64, texto: &str, color: &str, segundos: u32) -> Value {
        construir_mensaje_pantalla(id, texto, color, segundos)
    }

    fn redactar(&self, text: &str) -> String {
        match self.config.sala() {
            Some(sala) => redact(text, sala),
            None => text.to_string(),
        }
    }

    /// Conecta al relay con backoff exponencial para absorber cold starts de Render.
    async fn conectar_con_backoff(&self) -> Result<WsStream> {
        let url = self.construir_url_relay();
        let destino = self.destino();
        let mut ultimo_error = String::from("sin detalle");

        for intento in 1..=MAX_CONEXION_REINTENTOS {
            eprintln!(
                "[{}] Juego: conectando al relay para {destino} (intento {intento}/{MAX_CONEXION_REINTENTOS})...",
                now_iso()
            );

            match timeout(
                Duration::from_secs(TIMEOUT_CONEXION_SECS),
                connect_async(&url),
            )
            .await
            {
                Ok(Ok((mut ws, _response))) => {
                    self.leer_handshake(&mut ws).await;
                    return Ok(ws);
                }
                Ok(Err(error)) => {
                    ultimo_error = self.redactar(&error.to_string());
                }
                Err(_) => {
                    ultimo_error = format!(
                        "timeout de {TIMEOUT_CONEXION_SECS}s esperando al relay (¿cold start de Render?)"
                    );
                }
            }

            eprintln!("[{}] Juego: fallo de conexión: {ultimo_error}", now_iso());

            if intento < MAX_CONEXION_REINTENTOS {
                let espera = espera_backoff(intento);
                eprintln!("[{}] Juego: reintentando en {espera}s...", now_iso());
                sleep(Duration::from_secs(espera)).await;
            }
        }

        bail!(
            "No se pudo conectar al relay tras {MAX_CONEXION_REINTENTOS} intentos: {ultimo_error}"
        )
    }

    /// El relay envía el estado de la sala nada más aceptar la conexión.
    async fn leer_handshake(&self, ws: &mut WsStream) {
        match timeout(Duration::from_secs(TIMEOUT_HANDSHAKE_SECS), ws.next()).await {
            Ok(Some(Ok(Message::Text(raw)))) => {
                let Ok(value) = serde_json::from_str::<Value>(&raw) else {
                    return;
                };
                if value.get("tipo").and_then(Value::as_str) != Some("sistema") {
                    return;
                }

                if value
                    .get("juego_conectado")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    eprintln!(
                        "[{}] Juego: conectado al relay y sincronizado con {}.",
                        now_iso(),
                        self.destino()
                    );
                } else {
                    eprintln!(
                        "[{}] Juego: conectado al relay ({}, esperando que el juego entre).",
                        now_iso(),
                        self.destino()
                    );
                }
            }
            Ok(Some(Ok(_))) => {}
            Ok(Some(Err(error))) => eprintln!(
                "[{}] Juego: error en handshake: {}",
                now_iso(),
                self.redactar(&error.to_string())
            ),
            Ok(None) => eprintln!(
                "[{}] Juego: el relay cerró la conexión durante el handshake.",
                now_iso()
            ),
            Err(_) => eprintln!(
                "[{}] Juego: sin handshake del relay en {TIMEOUT_HANDSHAKE_SECS}s; se continúa.",
                now_iso()
            ),
        }
    }

    /// Envía un mensaje a la pantalla del juego. Si la conexión se cayó, reconecta con backoff.
    pub async fn enviar_mensaje(&self, texto: &str, color: &str, segundos: u32) -> Result<()> {
        let mut guard = self.stream.lock().await;
        let mut ultimo_error = None;

        for intento in 1..=MAX_INTENTOS_ENVIO {
            if guard.is_none() {
                match self.conectar_con_backoff().await {
                    Ok(ws) => *guard = Some(ws),
                    Err(error) => return Err(error),
                }
            }

            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let payload = Self::construir_mensaje_pantalla(id, texto, color, segundos);
            let Some(ws) = guard.as_mut() else {
                unreachable!("la conexión se acaba de crear");
            };

            match Self::enviar_y_esperar(ws, id, payload).await {
                Ok(()) => {
                    eprintln!(
                        "[{}] Juego: mensaje entregado ({}).",
                        now_iso(),
                        self.destino()
                    );
                    return Ok(());
                }
                Err(EnvioError::Aplicacion(error)) => {
                    return Err(anyhow!("{}", self.redactar(&format!("{error:#}"))));
                }
                Err(EnvioError::Conexion(error)) => {
                    *guard = None;
                    ultimo_error = Some(error);
                    eprintln!(
                        "[{}] Juego: conexión perdida ({}); se reconectará.",
                        now_iso(),
                        self.redactar(&format!("{:#}", ultimo_error.as_ref().unwrap()))
                    );

                    if intento < MAX_INTENTOS_ENVIO {
                        let espera = espera_backoff(intento);
                        eprintln!("[{}] Juego: reintentando envío en {espera}s...", now_iso());
                        sleep(Duration::from_secs(espera)).await;
                    }
                }
            }
        }

        let error = ultimo_error.unwrap_or_else(|| anyhow!("no se pudo enviar el mensaje"));
        Err(anyhow!("{}", self.redactar(&format!("{error:#}"))))
    }

    async fn enviar_y_esperar(
        ws: &mut WsStream,
        id: u64,
        payload: Value,
    ) -> Result<(), EnvioError> {
        ws.send(Message::Text(payload.to_string().into()))
            .await
            .map_err(|error| EnvioError::Conexion(error.into()))?;

        let deadline = Instant::now() + Duration::from_secs(TIMEOUT_RESPUESTA_SECS);

        loop {
            let restante = deadline.saturating_duration_since(Instant::now());
            if restante.is_zero() {
                // El mensaje ya salió; no repetirlo solo porque falta el acuse.
                eprintln!(
                    "[{}] Juego: mensaje enviado (sin acuse en {TIMEOUT_RESPUESTA_SECS}s).",
                    now_iso()
                );
                return Ok(());
            }

            match timeout(restante, ws.next()).await {
                Err(_) => {
                    eprintln!(
                        "[{}] Juego: mensaje enviado (sin acuse en {TIMEOUT_RESPUESTA_SECS}s).",
                        now_iso()
                    );
                    return Ok(());
                }
                Ok(None) => {
                    return Err(EnvioError::Conexion(anyhow!(
                        "el relay cerró la conexión tras enviar"
                    )));
                }
                Ok(Some(Err(error))) => return Err(EnvioError::Conexion(error.into())),
                Ok(Some(Ok(Message::Text(raw)))) => {
                    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
                        continue;
                    };

                    // Ignora avisos de sistema y eventos no solicitados del juego.
                    if value.get("id").and_then(Value::as_u64) != Some(id) {
                        continue;
                    }

                    if value.get("ok").and_then(Value::as_bool) == Some(false) {
                        let error = value
                            .get("error")
                            .and_then(Value::as_str)
                            .unwrap_or("error desconocido reportado por el juego");
                        return Err(EnvioError::Aplicacion(anyhow!("{error}")));
                    }

                    return Ok(());
                }
                Ok(Some(Ok(Message::Close(_)))) => {
                    return Err(EnvioError::Conexion(anyhow!(
                        "el relay cerró la conexión tras enviar"
                    )));
                }
                Ok(Some(Ok(_))) => {}
            }
        }
    }
}

fn espera_backoff(intento: usize) -> u64 {
    2u64.saturating_pow(intento as u32).min(30)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_construir_mensaje_pantalla() {
        let msg = JuegoClient::construir_mensaje_pantalla(42, "Alerta solar", "#ffcc00", 8);
        assert_eq!(msg["id"], 42);
        assert_eq!(msg["cmd"], "mensaje");
        assert_eq!(msg["args"]["texto"], "Alerta solar");
        assert_eq!(msg["args"]["color"], "#ffcc00");
        assert_eq!(msg["args"]["segundos"], 8);
        assert!(msg.get("comando").is_none());
        assert!(msg.get("parametros").is_none());
    }

    #[test]
    fn test_construir_url_relay() {
        let config = JuegoConfig {
            enabled: true,
            relay_url: "wss://mission-trainer-relay.onrender.com/".to_string(),
            sala: Some("mi-sala".to_string()),
            color: "#ffcc00".to_string(),
            segundos: 8,
        };
        let client = JuegoClient::new(config);
        let url = client.construir_url_relay();
        assert_eq!(
            url,
            "wss://mission-trainer-relay.onrender.com/mcp?sala=mi-sala"
        );
    }

    #[test]
    fn test_construir_url_relay_global() {
        let config = JuegoConfig {
            enabled: true,
            relay_url: "wss://mission-trainer-relay.onrender.com".to_string(),
            sala: None,
            color: "#ffcc00".to_string(),
            segundos: 8,
        };
        let client = JuegoClient::new(config);
        assert_eq!(
            client.construir_url_relay(),
            "wss://mission-trainer-relay.onrender.com/mcp"
        );
    }

    #[test]
    fn test_construir_url_relay_sin_slash() {
        let config = JuegoConfig {
            enabled: true,
            relay_url: "ws://localhost:10000".to_string(),
            sala: Some("test_room".to_string()),
            color: "#ffcc00".to_string(),
            segundos: 5,
        };
        let client = JuegoClient::new(config);
        let url = client.construir_url_relay();
        assert_eq!(url, "ws://localhost:10000/mcp?sala=test_room");
    }

    #[test]
    fn test_backoff_exponencial() {
        assert_eq!(espera_backoff(1), 2);
        assert_eq!(espera_backoff(2), 4);
        assert_eq!(espera_backoff(3), 8);
        assert_eq!(espera_backoff(4), 16);
        assert_eq!(espera_backoff(5), 30);
        assert_eq!(espera_backoff(10), 30);
    }
}
