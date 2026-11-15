use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::domain::evento::NuevoEvento;

pub fn map_event(endpoint: &str, item: &Value) -> Result<NuevoEvento> {
    let donki_id = extract_id(endpoint, item)
        .ok_or_else(|| anyhow!("el evento no tiene identificador DONKI"))?;

    Ok(NuevoEvento {
        donki_id,
        tipo: endpoint.to_string(),
        intensidad: extract_intensity(endpoint, item),
        inicio: extract_start_time(item),
        datos: item.clone(),
    })
}

fn extract_id(endpoint: &str, item: &Value) -> Option<String> {
    let key = match endpoint {
        "CME" | "IPS" => "activityID",
        "GST" => "gstID",
        "SEP" => "sepID",
        "FLR" => "flrID",
        _ => "activityID",
    };

    item.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            item.get("activityID")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
        })
        .map(ToString::to_string)
}

fn extract_start_time(item: &Value) -> Option<DateTime<Utc>> {
    ["startTime", "beginTime", "eventTime", "peakTime"]
        .iter()
        .find_map(|key| {
            item.get(*key)
                .and_then(Value::as_str)
                .and_then(parse_donki_time)
        })
}

fn parse_donki_time(value: &str) -> Option<DateTime<Utc>> {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Some(parsed.with_timezone(&Utc));
    }

    let without_z = value.trim_end_matches('Z');

    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S%.f",
    ] {
        if let Ok(parsed) = chrono::NaiveDateTime::parse_from_str(without_z, format) {
            return Some(parsed.and_utc());
        }
    }

    None
}

fn extract_intensity(endpoint: &str, item: &Value) -> Option<String> {
    match endpoint {
        "FLR" => item
            .get("classType")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
        "GST" => {
            let max_kp = item
                .get("allKpIndex")
                .and_then(Value::as_array)
                .and_then(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.get("kpIndex").and_then(Value::as_f64))
                        .reduce(f64::max)
                });

            max_kp.map(|kp| format!("Kp {}", format_number(kp)))
        }
        "CME" => {
            let analyses = item.get("cmeAnalyses").and_then(Value::as_array);
            let most_accurate = analyses.and_then(|items| {
                items
                    .iter()
                    .find(|item| item.get("isMostAccurate").and_then(Value::as_bool) == Some(true))
                    .or_else(|| items.first())
            });

            most_accurate
                .and_then(|analysis| analysis.get("speed"))
                .and_then(Value::as_f64)
                .map(|speed| format!("{} km/s", format_number(speed)))
        }
        _ => None,
    }
}

fn format_number(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        return format!("{value:.0}");
    }

    let mut formatted = format!("{value:.2}");

    while formatted.ends_with('0') {
        formatted.pop();
    }

    if formatted.ends_with('.') {
        formatted.pop();
    }

    formatted
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::SecondsFormat;
    use serde_json::json;

    #[test]
    fn parses_donki_times_with_and_without_seconds() {
        let with_seconds = parse_donki_time("2026-11-13T01:26:00Z").unwrap();
        let without_seconds = parse_donki_time("2026-11-13T01:26Z").unwrap();

        assert_eq!(
            with_seconds.to_rfc3339_opts(SecondsFormat::Secs, true),
            "2026-11-13T01:26:00Z"
        );
        assert_eq!(without_seconds, with_seconds);
    }

    #[test]
    fn extracts_ids_from_each_endpoint() {
        assert_eq!(
            extract_id("CME", &json!({"activityID": "cme-1"})).as_deref(),
            Some("cme-1")
        );
        assert_eq!(
            extract_id("GST", &json!({"gstID": "gst-1"})).as_deref(),
            Some("gst-1")
        );
        assert_eq!(
            extract_id("SEP", &json!({"sepID": "sep-1"})).as_deref(),
            Some("sep-1")
        );
        assert_eq!(
            extract_id("FLR", &json!({"flrID": "flr-1"})).as_deref(),
            Some("flr-1")
        );
        assert_eq!(
            extract_id("IPS", &json!({"activityID": "ips-1"})).as_deref(),
            Some("ips-1")
        );
    }

    #[test]
    fn extracts_intensity_from_flr_gst_and_cme() {
        assert_eq!(
            extract_intensity("FLR", &json!({"classType": "M2.5"})).as_deref(),
            Some("M2.5")
        );
        assert_eq!(
            extract_intensity(
                "GST",
                &json!({"allKpIndex": [{"kpIndex": 4}, {"kpIndex": 7}]})
            )
            .as_deref(),
            Some("Kp 7")
        );
        assert_eq!(
            extract_intensity(
                "CME",
                &json!({
                    "cmeAnalyses": [
                        {"isMostAccurate": false, "speed": 100.0},
                        {"isMostAccurate": true, "speed": 525.0}
                    ]
                })
            )
            .as_deref(),
            Some("525 km/s")
        );
    }

    #[test]
    fn maps_an_event_to_the_database_shape() {
        let event = map_event(
            "CME",
            &json!({
                "activityID": "2026-11-13T03:24:00-CME-001",
                "startTime": "2026-11-13T03:24Z",
                "cmeAnalyses": [{"isMostAccurate": true, "speed": 525.0}]
            }),
        )
        .unwrap();

        assert_eq!(event.donki_id, "2026-11-13T03:24:00-CME-001");
        assert_eq!(event.tipo, "CME");
        assert_eq!(event.inicio, parse_donki_time("2026-11-13T03:24Z"));
        assert_eq!(event.intensidad.as_deref(), Some("525 km/s"));
    }
}
