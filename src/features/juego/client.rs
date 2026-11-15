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

use chrono::SecondsFormat;

use crate::domain::evento::Evento;
use crate::shared::text::{now_iso, redact};

use super::config::JuegoConfig;

pub const MAX_MENSAJE_CHARS: usize = 160;
const MAX_CONEXION_REINTENTOS: usize = 4;
const TIMEOUT_CONEXION_SECS: u64 = 60;
const TIMEOUT_HANDSHAKE_SECS: u64 = 10;
const TIMEOUT_RESPUESTA_SECS: u64 = 15;
const MAX_INTENTOS_ENVIO: usize = 2;

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

// Protocolo del juego (scripts/conexion_mcp.gd): recibe {"id", "cmd", "args"} y
// responde {"id", "ok", "datos" | "error"}. Comandos usados aquí:
//   "mensaje"     texto en pantalla (args: texto, color, segundos)
//   "evento_nasa" evento de clima espacial con su mecánica (scripts/eventos_nasa.gd;
//                 args: tipo, intensidad, fecha, mensaje, dificultad, evento_id)
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

/// Evento real de la NASA para el juego: dispara su mecánica (radiación, reparaciones,
/// fallas eléctricas...) y muestra el mensaje de la IA.
pub fn construir_evento_nasa(id: u64, evento: &Evento, mensaje: &str, dificultad: &str) -> Value {
    serde_json::json!({
        "id": id,
        "cmd": "evento_nasa",
        "args": {
            "tipo": evento.tipo,
            "intensidad": evento.intensidad.clone().unwrap_or_default(),
            "fecha": evento
                .inicio
                .map(|f| f.to_rfc3339_opts(SecondsFormat::Secs, true))
                .unwrap_or_default(),
            "mensaje": mensaje,
            "dificultad": dificultad,
            "evento_id": evento.id,
        }
    })
}

// El relay de Mission Trainer tiene dos entradas para el listener:
//   /mcp?sala=CODIGO          una sola sala (la sala va en la query)
//   /difusion?clave=CLAVE     todos los juegos de todas las salas
// No existe /mcp sin sala: el relay rechaza esa conexión.
pub fn construir_url_relay(relay_url: &str, sala: Option<&str>, clave: Option<&str>) -> Result<String> {
    let base = relay_url.trim_end_matches('/');
    match (sala, clave) {
        (Some(sala), _) => Ok(format!("{base}/mcp?sala={sala}")),
        (None, Some(clave)) => Ok(format!("{base}/difusion?clave={}", codificar(clave))),
        (None, None) => bail!("Falta JUEGO_SALA o DIFUSION_CLAVE para enviar al juego"),
    }
}

/// Codifica la clave para la query (?clave=...).
fn codificar(texto: &str) -> String {
    texto
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
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

    pub fn construir_url_relay(&self) -> Result<String> {
        construir_url_relay(
            &self.config.relay_url,
            self.config.sala(),
            self.config.clave_difusion.as_deref(),
        )
    }

    fn es_difusion(&self) -> bool {
        self.config.sala().is_none()
    }

    fn destino(&self) -> &'static str {
        // La sala y la clave funcionan como contraseñas; nunca se imprimen en los logs.
        if self.es_difusion() {
            "todas las salas (difusión)"
        } else {
            "la sala configurada"
        }
    }

    fn redactar(&self, text: &str) -> String {
        let mut texto = text.to_string();
        if let Some(sala) = self.config.sala() {
            texto = redact(&texto, sala);
        }
        if let Some(clave) = self.config.clave_difusion.as_deref() {
            texto = redact(&texto, clave);
            texto = redact(&texto, &codificar(clave));
        }
        texto
    }

    /// Conecta al relay con backoff exponencial para absorber cold starts de Render.
    async fn conectar_con_backoff(&self) -> Result<WsStream> {
        let url = self.construir_url_relay()?;
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
                    // /difusion no manda estado al conectar; /mcp?sala= sí.
                    if !self.es_difusion() {
                        self.leer_handshake(&mut ws).await;
                    }
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

    /// Envía un texto a la pantalla del juego.
    pub async fn enviar_mensaje(&self, texto: &str, color: &str, segundos: u32) -> Result<()> {
        self.enviar_comando(|id| construir_mensaje_pantalla(id, texto, color, segundos))
            .await
    }

    /// Envía un evento real de la NASA: el juego ejecuta su mecánica y muestra el mensaje.
    pub async fn enviar_evento(&self, evento: &Evento, mensaje: &str) -> Result<()> {
        self.enviar_comando(|id| construir_evento_nasa(id, evento, mensaje, &self.config.dificultad))
            .await
    }

    /// Envía un comando al relay. Si la conexión se cayó, reconecta con backoff.
    async fn enviar_comando(&self, construir: impl Fn(u64) -> Value) -> Result<()> {
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
            let payload = construir(id);
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

                    // /difusion confirma con {"tipo": "difusion", "juegos": N}
                    if value.get("tipo").and_then(Value::as_str) == Some("difusion") {
                        let juegos = value.get("juegos").and_then(Value::as_u64).unwrap_or(0);
                        eprintln!("[{}] Juego: el relay lo mandó a {juegos} juego(s).", now_iso());
                        return Ok(());
                    }

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
        let msg = construir_mensaje_pantalla(42, "Alerta solar", "#ffcc00", 8);
        assert_eq!(msg["id"], 42);
        assert_eq!(msg["cmd"], "mensaje");
        assert_eq!(msg["args"]["texto"], "Alerta solar");
        assert_eq!(msg["args"]["color"], "#ffcc00");
        assert_eq!(msg["args"]["segundos"], 8);
        assert!(msg.get("comando").is_none());
        assert!(msg.get("parametros").is_none());
    }

    fn config(relay: &str, sala: Option<&str>, clave: Option<&str>) -> JuegoConfig {
        JuegoConfig {
            enabled: true,
            relay_url: relay.to_string(),
            sala: sala.map(str::to_string),
            color: "#ffcc00".to_string(),
            segundos: 8,
            clave_difusion: clave.map(str::to_string),
            dificultad: "Normal".to_string(),
        }
    }

    #[test]
    fn test_construir_url_relay() {
        let client = JuegoClient::new(config("wss://mission-trainer-relay.onrender.com/", Some("mi-sala"), None));
        assert_eq!(
            client.construir_url_relay().unwrap(),
            "wss://mission-trainer-relay.onrender.com/mcp?sala=mi-sala"
        );
    }

    #[test]
    fn test_construir_url_relay_difusion() {
        let client = JuegoClient::new(config("wss://mission-trainer-relay.onrender.com", None, Some("a+b/c=")));
        assert_eq!(
            client.construir_url_relay().unwrap(),
            "wss://mission-trainer-relay.onrender.com/difusion?clave=a%2Bb%2Fc%3D"
        );
    }

    #[test]
    fn test_construir_url_relay_sin_destino() {
        let client = JuegoClient::new(config("wss://mission-trainer-relay.onrender.com", None, None));
        assert!(client.construir_url_relay().is_err());
    }

    #[test]
    fn test_construir_url_relay_sin_slash() {
        let client = JuegoClient::new(config("ws://localhost:10000", Some("test_room"), None));
        assert_eq!(client.construir_url_relay().unwrap(), "ws://localhost:10000/mcp?sala=test_room");
    }

    #[test]
    fn test_construir_evento_nasa() {
        use chrono::{TimeZone, Utc};
        let evento = Evento {
            id: 37,
            donki_id: None,
            tipo: "FLR".into(),
            intensidad: Some("C2.8".into()),
            inicio: Some(Utc.with_ymd_and_hms(2026, 11, 14, 21, 16, 0).unwrap()),
            datos: Value::Null,
        };
        let msg = construir_evento_nasa(7, &evento, "Fulguración detectada", "Difícil");
        assert_eq!(msg["cmd"], "evento_nasa");
        assert_eq!(msg["args"]["tipo"], "FLR");
        assert_eq!(msg["args"]["intensidad"], "C2.8");
        assert_eq!(msg["args"]["fecha"], "2026-11-14T21:16:00Z");
        assert_eq!(msg["args"]["evento_id"], 37);
        assert_eq!(msg["args"]["dificultad"], "Difícil");
    }

    /// Contra un relay local: RELAY_PRUEBA=ws://127.0.0.1:10000 CLAVE_PRUEBA=... cargo test
    #[tokio::test]
    async fn test_difusion_a_relay_local() {
        let (Ok(relay), Ok(clave)) = (std::env::var("RELAY_PRUEBA"), std::env::var("CLAVE_PRUEBA")) else {
            return;
        };
        let client = JuegoClient::new(config(&relay, None, Some(&clave)));
        client.enviar_mensaje("Prueba de difusión", "#ffcc00", 5).await.unwrap();
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
