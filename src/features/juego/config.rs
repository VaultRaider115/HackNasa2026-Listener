use anyhow::{Result, bail};

use crate::shared::env::{env_bool_or, env_parse, first_non_empty};

pub const JUEGO_RELAY_URL_DEFAULT: &str = "wss://mission-trainer-relay.onrender.com";
pub const JUEGO_COLOR_DEFAULT: &str = "#ffcc00";
pub const JUEGO_SEGUNDOS_DEFAULT: u32 = 8;
pub const JUEGO_DIFICULTAD_DEFAULT: &str = "Normal";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JuegoConfig {
    pub enabled: bool,
    pub relay_url: String,
    pub sala: Option<String>,
    pub color: String,
    pub segundos: u32,
    /// Clave de /difusion del relay (DIFUSION_CLAVE en Render). Se usa cuando no hay
    /// JUEGO_SALA: el evento llega a todos los juegos de todas las salas.
    pub clave_difusion: Option<String>,
    /// Dificultad con la que el juego ejecuta el evento: Fácil, Normal, Difícil o Extremo.
    pub dificultad: String,
}

impl JuegoConfig {
    pub fn from_env() -> Result<Self> {
        let enabled = env_bool_or("JUEGO_ENABLED", false);
        let relay_url = first_non_empty(&["JUEGO_RELAY_URL"])
            .unwrap_or_else(|| JUEGO_RELAY_URL_DEFAULT.to_string());
        validar_relay_url(&relay_url)?;

        // Con JUEGO_SALA: solo esa sala (/mcp?sala=...).
        // Sin JUEGO_SALA: difusión a todas las salas (/difusion?clave=DIFUSION_CLAVE).
        let sala = first_non_empty(&["JUEGO_SALA"]);
        if let Some(s) = sala.as_deref() {
            validar_sala(s)?;
        }
        let clave_difusion = first_non_empty(&["DIFUSION_CLAVE"]);
        if enabled && sala.is_none() && clave_difusion.is_none() {
            bail!(
                "JUEGO_ENABLED=true necesita JUEGO_SALA (una sala) o DIFUSION_CLAVE (todas las salas)"
            );
        }
        let dificultad = first_non_empty(&["JUEGO_DIFICULTAD"])
            .unwrap_or_else(|| JUEGO_DIFICULTAD_DEFAULT.to_string());

        let color =
            first_non_empty(&["JUEGO_COLOR"]).unwrap_or_else(|| JUEGO_COLOR_DEFAULT.to_string());

        let segundos = env_parse::<u32>("JUEGO_SEGUNDOS", JUEGO_SEGUNDOS_DEFAULT)?;
        if segundos == 0 {
            bail!("JUEGO_SEGUNDOS debe ser mayor que 0");
        }

        Ok(Self {
            enabled,
            relay_url,
            sala,
            color,
            segundos,
            clave_difusion,
            dificultad,
        })
    }

    pub fn sala(&self) -> Option<&str> {
        self.sala.as_deref()
    }
}

pub fn validar_relay_url(url: &str) -> Result<()> {
    if !url.starts_with("ws://") && !url.starts_with("wss://") {
        bail!("JUEGO_RELAY_URL debe empezar por ws:// o wss://");
    }
    Ok(())
}

pub fn validar_sala(sala: &str) -> Result<()> {
    if sala.len() < 3 || sala.len() > 40 {
        bail!(
            "JUEGO_SALA debe tener entre 3 y 40 caracteres (tiene {})",
            sala.len()
        );
    }
    if !sala
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("JUEGO_SALA solo puede contener letras, números, '-' o '_'");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validar_sala_valida() {
        assert!(validar_sala("abc").is_ok());
        assert!(validar_sala("sala-123_test").is_ok());
        assert!(validar_sala("A".repeat(40).as_str()).is_ok());
    }

    #[test]
    fn test_validar_sala_invalida_longitud() {
        assert!(validar_sala("ab").is_err());
        assert!(validar_sala("").is_err());
        assert!(validar_sala("A".repeat(41).as_str()).is_err());
    }

    #[test]
    fn test_validar_sala_caracteres_invalidos() {
        assert!(validar_sala("sala con espacios").is_err());
        assert!(validar_sala("sala!").is_err());
        assert!(validar_sala("sala@123").is_err());
        assert!(validar_sala("sala/test").is_err());
    }

    #[test]
    fn test_validar_relay_url() {
        assert!(validar_relay_url("wss://mission-trainer-relay.onrender.com").is_ok());
        assert!(validar_relay_url("ws://localhost:10000").is_ok());
        assert!(validar_relay_url("https://mission-trainer-relay.onrender.com").is_err());
        assert!(validar_relay_url("mission-trainer-relay.onrender.com").is_err());
    }
}
