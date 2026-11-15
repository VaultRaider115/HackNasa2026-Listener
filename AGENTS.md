# Contexto del proyecto

Listener en Rust que consulta NASA DONKI, guarda eventos en Postgres (Supabase)
con SQLx y genera mensajes con un proveedor de IA compatible con OpenAI.

## Idioma y estilo

- Responde siempre en español, breve y directo.
- Cambios mínimos: arregla la causa raíz una vez en el código compartido.
- Mantén la estructura existente: `domain`, `features`, `infrastructure`,
  `shared`. No añadas capas, traits ni configuración que nadie pidió.
- Comenta solo el porqué no evidente, en una línea.

## Comandos

Toolchain del entorno:

```bash
export PATH=/workspace/.tools/x86_64-linux-musl-cross/bin:/workspace/.tools/rust/cargo/bin:$PATH
export CARGO_HOME=/workspace/.tools/rust/cargo
export RUSTUP_HOME=/workspace/.tools/rust/rustup
export CC=x86_64-linux-musl-cc
export AR=x86_64-linux-musl-ar
export RUSTFLAGS='-C linker=x86_64-linux-musl-cc'
```

Antes de terminar cualquier cambio:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets
```

Pruebas manuales:

```bash
cargo run -- --once --dry-run   # NASA sin escribir
cargo run -- --once             # NASA contra la base real
cargo run -- --ai-once          # un envío a la IA
cargo run -- --ai-only          # solo worker IA
```

`--ai-once` y `--ai-only` se pueden probar sin `API_NASA_KEY`.

## Secretos

- `api.env` contiene `API_IA_KEY`, `API_NASA_KEY` y `DATABASE_URL` reales.
- Nunca imprimas ni copies valores de `api.env`, `.env` ni `DATABASE_URL`.
- No subas `.env` ni `api.env` al repo (ya están en `.gitignore`).
- Al mostrar errores de conexión, redáctalos; no incluyas la cadena completa.

## Base de datos

- Usa SQLx directo contra Postgres. No hay cliente REST de Supabase.
- Conecta por conexión directa o Session pooler (puerto 5432). El Transaction
  pooler (6543) no soporta prepared statements de SQLx.
- Índices únicos obligatorios (ver `migrations/20261114_unique_constraints.sql`):
  - `mensajes_ia(evento_id)`
  - `sync_nasa(endpoint)`
- Los inserts usan `ON CONFLICT`; no los cambies por `SELECT` + `INSERT`.
- No borres ni modifiques filas reales sin pedir confirmación antes.
- Migraciones nuevas van en `migrations/` con fecha en el nombre.

## Reglas del dominio

- El contexto que guía a la IA del juego vive en `contexto_ia.md`; edítalo ahí,
  no dentro del código. `AI_PROMPT` lo reemplaza si está definido.
- Endpoints NASA: `CME`, `GST`, `SEP`, `FLR`, `IPS`.
- `FLR` usa `https://ccmc.gsfc.nasa.gov/DONKI-API/get/FLR` sin API key; el
  resto usa `NASA_API_BASE` y sí requiere `API_NASA_KEY`.
- `API_NASA_KEY` es opcional: solo se exige al consultar NASA.
- Un fallo de escritura en Postgres no debe reportarse como fallo de NASA
  (`EndpointError::{Nasa, Write}`).
- El worker IA elige un evento pendiente al azar y guarda con
  `ON CONFLICT DO NOTHING`; si otra instancia ya guardó, no es un error.
- Si el proveedor IA responde 402/429/401 o falla la red, el evento queda
  pendiente para el siguiente ciclo.

## Documentación

`README.md` describe arquitectura, variables y comportamiento. Si cambias
configuración, endpoints o requisitos de la base, actualiza también el README y
`.env.example`.
