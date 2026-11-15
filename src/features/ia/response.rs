use serde_json::Value;

use crate::domain::mensaje_ia::ContenidoIa;

pub(super) fn parse(content: &str) -> ContenidoIa {
    let trimmed = content.trim();
    let json_text = strip_markdown_fence(trimmed);

    if let Some(contenido) = parse_json(json_text) {
        return contenido;
    }

    ContenidoIa {
        mensaje: trimmed.to_string(),
        explicacion: None,
    }
}

fn parse_json(text: &str) -> Option<ContenidoIa> {
    let value: Value = serde_json::from_str(text).ok().or_else(|| {
        let start = text.find('{')?;
        let end = text.rfind('}')?;
        if end <= start {
            return None;
        }
        serde_json::from_str(&text[start..=end]).ok()
    })?;

    let mensaje = value
        .get("mensaje")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let explicacion = value
        .get("explicacion")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    Some(ContenidoIa {
        mensaje,
        explicacion,
    })
}

fn strip_markdown_fence(content: &str) -> &str {
    let trimmed = content.trim();

    if let Some(inner) = trimmed
        .strip_prefix("```")
        .and_then(|value| value.strip_suffix("```"))
    {
        let inner = inner.trim();
        let inner = inner
            .strip_prefix("json")
            .or_else(|| inner.strip_prefix("JSON"))
            .unwrap_or(inner);
        return inner.trim();
    }

    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_with_markdown_fence() {
        let contenido =
            parse("```json\n{\"mensaje\":\"Alerta\",\"explicacion\":\"Explicación\"}\n```");

        assert_eq!(contenido.mensaje, "Alerta");
        assert_eq!(contenido.explicacion.as_deref(), Some("Explicación"));
    }

    #[test]
    fn falls_back_to_raw_text() {
        let contenido = parse("Texto libre de la IA");

        assert_eq!(contenido.mensaje, "Texto libre de la IA");
        assert_eq!(contenido.explicacion, None);
    }

    #[test]
    fn extracts_json_surrounded_by_prose() {
        let contenido = parse(
            "Aquí tienes el resultado:\n{\"mensaje\":\"Alerta\",\"explicacion\":\"Explicación\"}\nListo.",
        );

        assert_eq!(contenido.mensaje, "Alerta");
        assert_eq!(contenido.explicacion.as_deref(), Some("Explicación"));
    }
}
