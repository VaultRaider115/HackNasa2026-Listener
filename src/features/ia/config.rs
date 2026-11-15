use anyhow::{Context, Result, bail};

use crate::shared::env::{env_bool_or, env_parse, first_non_empty};

use super::prompt::DEFAULT_PROMPT;

#[derive(Debug, Clone)]
pub struct AiConfig {
    pub disabled_reason: Option<String>,
    pub api_key: Option<String>,
    pub api_url: String,
    pub model: String,
    pub prompt: String,
    pub json_mode: bool,
    pub min_minutes: u64,
    pub max_minutes: u64,
    pub max_tokens: u32,
    pub temperature: f64,
    pub timeout_secs: u64,
    pub pending_limit: usize,
}

impl AiConfig {
    pub fn from_env() -> Result<Self> {
        let ai_enabled = env_bool_or("AI_ENABLED", true);
        let api_key = first_non_empty(&["AI_API_KEY", "API_IA_KEY"]);
        let api_url = first_non_empty(&["AI_API_URL"]);
        let model = first_non_empty(&["AI_MODEL"]);

        let disabled_reason = if !ai_enabled {
            Some("AI_ENABLED=false".to_string())
        } else if api_key.is_none() {
            Some("falta AI_API_KEY (o API_IA_KEY)".to_string())
        } else if api_url.is_none() {
            Some("falta AI_API_URL".to_string())
        } else if model.is_none() {
            Some("falta AI_MODEL".to_string())
        } else {
            None
        };

        if let Some(reason) = disabled_reason {
            return Ok(Self {
                disabled_reason: Some(reason),
                api_key,
                api_url: api_url.unwrap_or_default(),
                model: model.unwrap_or_default(),
                prompt: DEFAULT_PROMPT.to_string(),
                json_mode: true,
                min_minutes: 15,
                max_minutes: 60,
                max_tokens: 700,
                temperature: 0.8,
                timeout_secs: 120,
                pending_limit: 500,
            });
        }

        let api_url = api_url.context("Falta AI_API_URL")?;
        let model = model.context("Falta AI_MODEL")?;

        if !api_url.starts_with("http://") && !api_url.starts_with("https://") {
            bail!("AI_API_URL debe empezar por http:// o https://");
        }

        let min_minutes = env_parse::<u64>("AI_MIN_MINUTOS", 15)?;
        let max_minutes = env_parse::<u64>("AI_MAX_MINUTOS", 60)?;

        if min_minutes < 1 || max_minutes < min_minutes {
            bail!("AI_MIN_MINUTOS debe ser >= 1 y AI_MAX_MINUTOS debe ser >= AI_MIN_MINUTOS");
        }

        let max_tokens = env_parse::<u32>("AI_MAX_TOKENS", 700)?;
        if max_tokens == 0 {
            bail!("AI_MAX_TOKENS debe ser mayor que 0");
        }

        let temperature = env_parse::<f64>("AI_TEMPERATURE", 0.8)?;
        if !(0.0..=2.0).contains(&temperature) {
            bail!("AI_TEMPERATURE debe estar entre 0 y 2");
        }

        let timeout_secs = env_parse::<u64>("AI_TIMEOUT_SEGUNDOS", 120)?;
        if timeout_secs < 10 {
            bail!("AI_TIMEOUT_SEGUNDOS debe ser al menos 10");
        }

        let pending_limit = env_parse::<usize>("AI_PENDING_LIMIT", 500)?;
        if pending_limit == 0 {
            bail!("AI_PENDING_LIMIT debe ser mayor que 0");
        }

        Ok(Self {
            disabled_reason: None,
            api_key,
            api_url,
            model,
            prompt: first_non_empty(&["AI_PROMPT"]).unwrap_or_else(|| DEFAULT_PROMPT.to_string()),
            json_mode: env_bool_or("AI_JSON_MODE", true),
            min_minutes,
            max_minutes,
            max_tokens,
            temperature,
            timeout_secs,
            pending_limit,
        })
    }

    pub fn enabled(&self) -> bool {
        self.disabled_reason.is_none()
    }

    pub fn ensure_ready(&self) -> Result<()> {
        if let Some(reason) = &self.disabled_reason {
            bail!("IA deshabilitada: {reason}");
        }

        Ok(())
    }

    pub(crate) fn api_key(&self) -> Result<&str> {
        self.ensure_ready()?;

        self.api_key
            .as_deref()
            .context("Falta AI_API_KEY (o API_IA_KEY) para llamar a la IA")
    }
}
