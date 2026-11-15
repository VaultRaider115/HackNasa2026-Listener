use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
pub struct NuevoEvento {
    pub donki_id: String,
    pub tipo: String,
    pub intensidad: Option<String>,
    pub inicio: Option<DateTime<Utc>>,
    pub datos: Value,
}

#[derive(Debug, Serialize)]
pub struct Evento {
    pub id: i64,
    pub donki_id: Option<String>,
    pub tipo: String,
    pub intensidad: Option<String>,
    pub inicio: Option<DateTime<Utc>>,
    pub datos: Value,
}
