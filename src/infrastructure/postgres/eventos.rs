use anyhow::{Context, Result};
use sqlx::{PgPool, Postgres, QueryBuilder};

use crate::domain::evento::NuevoEvento;

pub async fn insert(pool: &PgPool, rows: &[NuevoEvento]) -> Result<usize> {
    if rows.is_empty() {
        return Ok(0);
    }

    let mut builder: QueryBuilder<Postgres> = QueryBuilder::new(
        "insert into public.eventos (donki_id, tipo, intensidad, inicio, datos) ",
    );

    builder.push_values(rows, |mut row, event| {
        row.push_bind(&event.donki_id)
            .push_bind(&event.tipo)
            .push_bind(&event.intensidad)
            .push_bind(event.inicio)
            .push_bind(&event.datos);
    });

    builder.push(" on conflict (donki_id) do nothing");

    let result = builder
        .build()
        .execute(pool)
        .await
        .context("No se pudieron insertar los eventos en Postgres")?;

    Ok(result.rows_affected() as usize)
}
