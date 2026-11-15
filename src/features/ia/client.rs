use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::domain::evento::Evento;
use crate::shared::text::{now_iso, redact, truncate};

use super::config::AiConfig;
use super::prompt::build_messages;

pub(super) async fn call(
    client: &reqwest::Client,
    config: &AiConfig,
    api_key: &str,
    event: &Evento,
) -> Result<String> {
    let messages = build_messages(&config.prompt, event)?;

    let mut payload = json!({
        "model": &config.model,
        "messages": messages,
        "temperature": config.temperature,
        "max_tokens": config.max_tokens,
        "stream": false,
    });

    if config.json_mode {
        payload["response_format"] = json!({"type": "json_object"});
    }

    let response = client
        .post(&config.api_url)
        .bearer_auth(api_key)
        .header("Content-Type", "application/json")
        .timeout(Duration::from_secs(config.timeout_secs))
        .json(&payload)
        .send()
        .await
        .context("No se pudo conectar con el proveedor de IA")?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        let message = truncate(&redact(&body, api_key), 800);

        match status.as_u16() {
            401 => bail!("El proveedor de IA rechazó la API key (HTTP 401): {message}"),
            402 => bail!(
                "El proveedor de IA informó de falta de saldo o cuota (HTTP 402). El evento queda pendiente para reintentarlo: {message}"
            ),
            429 => bail!(
                "El proveedor de IA limitó las peticiones o la cuota (HTTP 429). El evento queda pendiente para reintentarlo: {message}"
            ),
            _ => bail!("El proveedor de IA respondió HTTP {status}: {message}"),
        }
    }

    let parsed: Value = serde_json::from_str(&body).with_context(|| {
        format!(
            "El proveedor de IA devolvió una respuesta no JSON: {}",
            truncate(&body, 400)
        )
    })?;

    if let Some(tokens) = parsed
        .pointer("/usage/total_tokens")
        .and_then(Value::as_i64)
    {
        eprintln!("[{}] IA: tokens usados: {tokens}", now_iso());
    }

    extract_content(&parsed).context("El proveedor de IA no devolvió contenido en la respuesta")
}

fn extract_content(response: &Value) -> Option<String> {
    let content = response.pointer("/choices/0/message/content");

    if let Some(text) = content.and_then(Value::as_str) {
        let text = text.trim();

        if !text.is_empty() {
            return Some(text.to_string());
        }
    }

    if let Some(parts) = content.and_then(Value::as_array) {
        let text = parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| part.get("content").and_then(Value::as_str))
            })
            .collect::<Vec<_>>()
            .join("");

        if !text.trim().is_empty() {
            return Some(text);
        }
    }

    ["/choices/0/text", "/output_text"]
        .iter()
        .find_map(|pointer| response.pointer(pointer).and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_string_content() {
        let response = json!({
            "choices": [{"message": {"content": "Hola"}}]
        });

        assert_eq!(extract_content(&response).as_deref(), Some("Hola"));
    }

    #[test]
    fn extracts_content_parts() {
        let response = json!({
            "choices": [{
                "message": {
                    "content": [
                        {"type": "text", "text": "Hola "},
                        {"type": "text", "text": "mundo"}
                    ]
                }
            }]
        });

        assert_eq!(extract_content(&response).as_deref(), Some("Hola mundo"));
    }

    #[test]
    fn falls_back_to_choice_text() {
        let response = json!({
            "choices": [{"text": "Hola"}]
        });

        assert_eq!(extract_content(&response).as_deref(), Some("Hola"));
    }
}
