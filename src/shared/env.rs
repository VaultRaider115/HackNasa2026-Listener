use std::env;
use std::path::PathBuf;

use anyhow::{Result, anyhow};

pub fn load_env_files() {
    let mut candidates = Vec::new();

    if let Ok(current_dir) = env::current_dir() {
        candidates.push(current_dir.join(".env"));
        candidates.push(current_dir.join("api.env"));
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    candidates.push(manifest_dir.join(".env"));
    candidates.push(manifest_dir.join("api.env"));

    for path in candidates {
        if path.is_file() && dotenvy::from_path(&path).is_err() {
            eprintln!(
                "No se pudo cargar {}: revisa el formato del archivo",
                path.display()
            );
        }
    }
}

pub fn first_non_empty(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

pub fn env_bool_or(name: &str, default: bool) -> bool {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "si" | "sí"
        ),
        _ => default,
    }
}

pub fn env_parse<T>(name: &str, default: T) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => value
            .trim()
            .parse::<T>()
            .map_err(|error| anyhow!("{name} inválido: {error}")),
        _ => Ok(default),
    }
}
