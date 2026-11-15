use anyhow::{Context, Result};
use serde_json::Value;
use sqlx::{PgPool, Row};

use crate::domain::evento::Evento;
use crate::domain::mensaje_ia::ContenidoIa;

pub async fn pending_ids(pool: &PgPool, pending_limit: usize) -> Result<Vec<i64>> {
    sqlx::query_scalar(
        r#"
        select e.id
        from public.eventos e
        left join public.mensajes_ia m on m.evento_id = e.id
        where m.id is null
        order by e.id desc
        limit $1
        "#,
    )
    .bind(pending_limit as i64)
    .fetch_all(pool)
    .await
    .context("No se pudieron consultar los eventos pendientes de IA")
}

pub async fn fetch_by_id(pool: &PgPool, event_id: i64) -> Result<Option<Evento>> {
    let row = sqlx::query(
        r#"
        select id, donki_id, tipo, intensidad, inicio, datos
        from public.eventos
        where id = $1
        "#,
    )
    .bind(event_id)
    .fetch_optional(pool)
    .await
    .with_context(|| format!("No se pudo leer el evento {event_id}"))?;

    let Some(row) = row else {
        return Ok(None);
    };

    Ok(Some(Evento {
        id: row.try_get("id")?,
        donki_id: row.try_get("donki_id")?,
        tipo: row.try_get("tipo")?,
        intensidad: row.try_get("intensidad")?,
        inicio: row.try_get("inicio")?,
        datos: row
            .try_get::<Option<Value>, _>("datos")?
            .unwrap_or(Value::Null),
    }))
}

pub async fn insert(pool: &PgPool, evento_id: i64, contenido: &ContenidoIa) -> Result<bool> {
    let result = sqlx::query(
        r#"
        insert into public.mensajes_ia (evento_id, mensaje, explicacion)
        values ($1, $2, $3)
        on conflict (evento_id) do nothing
        "#,
    )
    .bind(evento_id)
    .bind(&contenido.mensaje)
    .bind(contenido.explicacion.as_deref())
    .execute(pool)
    .await
    .context("No se pudo guardar el mensaje IA en Postgres")?;

    Ok(result.rows_affected() > 0)
}
