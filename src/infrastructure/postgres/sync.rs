use anyhow::{Context, Result};
use sqlx::PgPool;

pub async fn record(
    pool: &PgPool,
    endpoint: &str,
    new_count: i32,
    error: Option<&str>,
) -> Result<()> {
    sqlx::query(
        r#"
        insert into public.sync_nasa (endpoint, ultima_consulta, eventos_nuevos, error)
        values ($1, now(), $2, $3)
        on conflict (endpoint) do update
        set ultima_consulta = excluded.ultima_consulta,
            eventos_nuevos = excluded.eventos_nuevos,
            error = excluded.error
        "#,
    )
    .bind(endpoint)
    .bind(new_count)
    .bind(error)
    .execute(pool)
    .await
    .with_context(|| format!("Actualizando sync_nasa ({endpoint})"))?;

    Ok(())
}
