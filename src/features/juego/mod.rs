pub mod client;
pub mod config;

pub use client::{JuegoClient, MAX_MENSAJE_CHARS, construir_evento_nasa, construir_mensaje_pantalla};
pub use config::JuegoConfig;
