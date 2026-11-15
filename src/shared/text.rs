use chrono::{SecondsFormat, Utc};

pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub fn redact(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        text.to_string()
    } else {
        text.replace(secret, "[REDACTED]")
    }
}

pub fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let mut truncated: String = text.chars().take(max_chars).collect();
    truncated.push_str("...");
    truncated
}
