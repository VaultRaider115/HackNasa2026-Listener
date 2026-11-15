# Listener NASA DONKI + mensajes IA

Listener en Rust que consulta la API **DONKI** de NASA y guarda los eventos en
Postgres (Supabase) con **SQLx**. Por defecto consulta **ayer y hoy** (UTC) y
repite el ciclo cada hora.

Además, un worker elige un evento pendiente cada **15–60 minutos aleatorios**,
lo envía a un **proveedor de IA compatible con OpenAI** y guarda la respuesta en
`mensajes_ia`.

## Qué hace

- Consulta los endpoints `CME`, `GST`, `SEP`, `FLR` e `IPS`.
- Toma la ventana de fechas `[hoy - (DIAS_VENTANA - 1), hoy]` en UTC.
  - `DIAS_VENTANA=2` → ayer y hoy.
  - `DIAS_VENTANA=3` → anteayer, ayer y hoy.
- Inserta en `eventos` los eventos nuevos, usando `donki_id` como clave de
  deduplicación.
- Actualiza `sync_nasa` por endpoint con la última consulta, cuántos eventos
  nuevos llegaron y el error (si hubo).
- Si un endpoint falla, los demás continúan procesándose.
- Cada 15–60 minutos aleatorios, elige un evento sin mensaje IA, lo envía al
  proveedor de IA y guarda `mensaje` y `explicacion` en `mensajes_ia`.
- Si `JUEGO_ENABLED=true`, envía el `mensaje` a la pantalla de **Mission
  Trainer** por el relay WebSocket, sin pasar por el MCP de Python.

## Estructura del proyecto

```text
src/
├── main.rs                  # Arranque: entorno, configuración, HTTP y pool Postgres.
├── app.rs                   # Orquestación de los modos NASA/IA y Ctrl+C.
├── cli.rs                   # Argumentos, ayuda y banner.
├── config.rs                # Configuración global y constantes.
├── domain/                  # Modelos de dominio.
│   ├── evento.rs            # NuevoEvento y Evento.
│   └── mensaje_ia.rs        # ContenidoIa.
├── features/
│   ├── nasa/                # Feature de escucha DONKI.
│   │   ├── client.rs        # Llamadas a NASA con reintentos.
│   │   └── mapping.rs       # Mapeo DONKI → eventos.
│   ├── ia/                  # Feature de mensajes IA.
│   │   ├── config.rs        # AiConfig y validación.
│   │   ├── client.rs        # Llamada al proveedor de IA.
│   │   ├── prompt.rs        # Prompt por defecto y mensajes.
│   │   └── response.rs      # Parseo de la respuesta JSON.
│   └── juego/               # Cliente WebSocket para Mission Trainer.
│       ├── config.rs        # JuegoConfig y validación de sala.
│       └── client.rs        # Conexión al relay, reintentos y envío de mensajes.
├── infrastructure/
│   └── postgres/            # Repositorios SQLx.
│       ├── eventos.rs       # Insert de eventos.
│       ├── mensajes.rs      # Eventos pendientes y mensajes IA.
│       └── sync.rs          # Actualización de sync_nasa.
└── shared/                  # Utilidades transversales.
    ├── env.rs               # Carga y parseo de variables de entorno.
    ├── http.rs              # Construcción del cliente HTTP.
    └── text.rs              # Fechas ISO, redacción y truncado.
```

`contexto_ia.md` es el contexto por defecto que recibe la IA: define su papel,
el tono, cómo interpretar cada evento y el formato JSON de salida.

`migrations/` contiene los índices únicos que necesita el listener para sus
`INSERT ... ON CONFLICT`.

## Requisitos

- Rust estable (`rustup`).
- Proyecto Supabase con las tablas `eventos`, `sync_nasa` y `mensajes_ia` (ya
  existen en este proyecto), los índices únicos de
  `migrations/20261114_unique_constraints.sql` y su cadena de conexión
  Postgres.
- API key de NASA (solo para el listener NASA): https://api.nasa.gov/
- API key del proveedor de IA y una URL compatible con el formato
  `chat/completions` de OpenAI (por ejemplo DeepSeek, OpenAI, Groq u
  OpenRouter). Si el proveedor está sin saldo, el worker reintentará en el
  siguiente ciclo.

## Configuración

1. Copia el ejemplo:

   ```bash
   cp .env.example .env
   ```

2. Rellena las variables:

   ```env
   # Si no usas api.env:
   # API_NASA_KEY=tu_api_key_de_nasa
   # AI_API_KEY=tu_api_key_del_proveedor
   AI_API_URL=https://api.deepseek.com/chat/completions
   AI_MODEL=deepseek-chat
   DATABASE_URL=postgresql://postgres.tu-proyecto:[TU_PASSWORD]@aws-0-tu-region.pooler.supabase.com:5432/postgres
   ```

   El código también carga `api.env`, así que puedes mantener ahí
   `API_NASA_KEY` y `API_IA_KEY` si ya las tienes. Los nombres
   `API_NASA_KEY`/`NASA_API_KEY` y `AI_API_KEY`/`API_IA_KEY` son aceptados.
   `AI_API_URL` y `AI_MODEL` son obligatorias para el worker de IA. Si defines
   la misma variable en `.env` y en `api.env`, gana `.env`; por eso, si ya
   tienes las keys en `api.env`, deja esas líneas comentadas en `.env`.

3. `DATABASE_URL` se obtiene en
   **Supabase → Project Settings → Database → Connection string**.
   Usa la conexión directa o el **Session pooler** (puerto 5432), no el
   **Transaction pooler** (puerto 6543): SQLx usa prepared statements y el
   pooler transaccional no los soporta. La conexión debe usar un rol con
   permiso de escritura (la cadena del dashboard ya usa el usuario `postgres`,
   que no está limitado por RLS). La URL contiene la contraseña; no la subas al
   repo.

`.env` y `api.env` están en `.gitignore` para evitar subir secretos. El
proyecto usa consultas SQLx en tiempo de ejecución, así que no necesita una
base de datos para compilar.

Aplica `migrations/20261114_unique_constraints.sql` si tu base todavía no tiene
esos índices. El listener usa `INSERT ... ON CONFLICT` en `eventos`,
`mensajes_ia` y `sync_nasa`, así que sin ellos fallará al escribir.

## Uso

Probar un solo ciclo sin escribir en la base de datos:

```bash
cargo run -- --once --dry-run
```

Ejecutar un solo ciclo y guardar en Postgres:

```bash
cargo run -- --once
```

Probar un envío a la IA (sin esperar el tiempo aleatorio y sin guardar):

```bash
cargo run -- --ai-once --dry-run
```

`--dry-run` evita guardar el mensaje, pero igual necesita `DATABASE_URL` para
leer un evento pendiente.

Enviar un evento a la IA y guardar el mensaje:

```bash
cargo run -- --ai-once
```

Ejecutar solo el worker de IA, sin consultar NASA (no necesita `API_NASA_KEY`):

```bash
cargo run -- --ai-only
```

Mantener todo corriendo (NASA + worker IA, Ctrl+C para detener):

```bash
cargo run
```

También puedes compilar en release:

```bash
cargo build --release
./target/release/listener_nasa_game
```

### Opciones

| Opción | Descripción |
| --- | --- |
| `--once` | Ejecuta un solo ciclo de NASA y termina. |
| `--dry-run` | No escribe en base de datos ni envía al relay; muestra la respuesta IA o el mensaje a enviar. |
| `--ai-once` | Elige un evento pendiente, lo envía al proveedor de IA una vez y termina. |
| `--ai-only` | Ejecuta solo el worker de IA, sin consultar NASA. |
| `--juego-test "texto"` | Envía un mensaje de prueba al juego y termina. No necesita `DATABASE_URL` ni `JUEGO_ENABLED`. Con `JUEGO_SALA` va a esa sala; sin ella, por el MCP global a todas las salas. |
| `-h`, `--help` | Muestra la ayuda. |

### Variables de entorno

| Variable | Por defecto | Descripción |
| --- | --- | --- |
| `API_NASA_KEY` / `NASA_API_KEY` | — | API key de NASA. Obligatoria para el listener NASA; no para `--ai-only`/`--ai-once`. |
| `AI_API_KEY` / `API_IA_KEY` | — | API key del proveedor de IA. Obligatoria para el worker IA. |
| `AI_API_URL` | — | URL completa del endpoint de chat compatible con OpenAI. Obligatoria. |
| `AI_MODEL` | — | Modelo del proveedor de IA. Obligatorio. |
| `DATABASE_URL` | — | Cadena de conexión Postgres (Supabase). Obligatoria salvo en dry-run de NASA. |
| `DIAS_VENTANA` | `2` | Días de la ventana (2 = ayer y hoy). |
| `INTERVALO_MINUTOS` | `60` | Minutos entre ciclos de NASA. |
| `DRY_RUN` | `false` | `true` para no escribir en la base de datos. |
| `NASA_API_BASE` | `https://api.nasa.gov` | URL base de NASA (útil para pruebas). |
| `AI_ENABLED` | `true` | Activa/desactiva el worker de IA. |
| `AI_MIN_MINUTOS` | `15` | Mínimo de minutos aleatorios entre envíos. |
| `AI_MAX_MINUTOS` | `60` | Máximo de minutos aleatorios entre envíos. |
| `AI_PROMPT` | `contexto_ia.md` | Prompt personalizado. Si se define, reemplaza el contexto por defecto. |
| `AI_JSON_MODE` | `true` | Pide respuesta JSON con `mensaje` y `explicacion`. |
| `AI_MAX_TOKENS` | `700` | Máximo de tokens de la respuesta. |
| `AI_TEMPERATURE` | `0.8` | Creatividad del modelo (0 a 2). |
| `AI_TIMEOUT_SEGUNDOS` | `120` | Timeout de la llamada al proveedor de IA. |
| `AI_PENDING_LIMIT` | `500` | Eventos pendientes recientes entre los que se elige al azar. |
| `JUEGO_ENABLED` | `false` | Activa el envío de alertas de IA a la pantalla de Mission Trainer. |
| `JUEGO_RELAY_URL` | `wss://mission-trainer-relay.onrender.com` | URL del relay WebSocket. |
| `JUEGO_SALA` | — | Código de sala (3–40 caracteres: letras, números, `-`, `_`). Si se define, el mensaje va solo a esa sala. Si se omite, se usa el MCP global (`/mcp` sin sala) y el relay reenvía el mensaje a todas las salas. |
| `JUEGO_COLOR` | `#ffcc00` | Color hex del mensaje en pantalla. |
| `JUEGO_SEGUNDOS` | `8` | Duración en segundos del mensaje en pantalla. |

## Worker de IA

El worker corre en segundo plano junto al listener de NASA, salvo que se use
`--ai-once` o `--ai-only`.

1. Espera un tiempo aleatorio entre `AI_MIN_MINUTOS` y `AI_MAX_MINUTOS`.
2. Consulta en `eventos` los eventos que **no** tienen fila en `mensajes_ia`.
3. Elige uno al azar entre los `AI_PENDING_LIMIT` más recientes.
4. Envía el evento completo al proveedor de IA.
5. Guarda `mensaje` y `explicacion` en `mensajes_ia` con el `evento_id`.

Si el proveedor responde `402` o `429` (sin saldo o cuota), `401` (key
inválida) o hay un error de red, el evento **no** se marca como procesado y se
reintenta en el siguiente ciclo aleatorio.

### Proveedores compatibles

El cliente usa el formato de chat de OpenAI (`messages`, `temperature`,
`max_tokens` y, opcionalmente, `response_format`). La URL debe ser el endpoint
completo de chat, no solo la base:

```env
AI_API_URL=https://api.deepseek.com/chat/completions
AI_MODEL=deepseek-chat
```

Para otro proveedor compatible, cambia la URL y el modelo. Por ejemplo:

```env
AI_API_URL=https://api.openai.com/v1/chat/completions
AI_MODEL=gpt-4o-mini
```

La respuesta puede venir en `choices[0].message.content` (texto o lista de
partes) o en `choices[0].text`; el cliente admite ambos formatos.

### Prompt personalizado

El contexto por defecto está en `contexto_ia.md` y se compila dentro del
binario: si lo editas, vuelve a compilar. Define el papel de la IA, el tono, la
interpretación de cada evento y el contrato JSON de salida.

`AI_PROMPT` es opcional y, si existe, reemplaza por completo a ese contexto.
Puede ser:

- Un **system prompt** (sin `{evento}`): el evento se envía como mensaje de
  usuario aparte.
- Un **prompt completo** con `{evento}`: el placeholder se reemplaza por el
  JSON del evento.

```env
AI_PROMPT=Eres el narrador de una nave espacial. Con este evento, escribe una alerta y una explicación. Devuelve JSON con las claves mensaje y explicacion: {evento}
```

Con `AI_JSON_MODE=true` se envía `response_format: json_object`; el prompt debe
mencionar JSON y devolver las claves `mensaje` y `explicacion`. Si tu prompt
devuelve texto libre, pon `AI_JSON_MODE=false`; el texto se guardará en
`mensaje` y `explicacion` quedará `null`.

## Mapeo a `eventos`

| Endpoint | `donki_id` | `inicio` | `intensidad` |
| --- | --- | --- | --- |
| `CME` | `activityID` | `startTime` | velocidad del análisis más preciso (`km/s`) |
| `GST` | `gstID` | `startTime` | Kp máximo (`Kp 7`) |
| `SEP` | `sepID` | `eventTime` | `null` |
| `FLR` | `flrID` | `beginTime` | `classType` (`M2.5`) |
| `IPS` | `activityID` | `eventTime` | `null` |

El JSON completo de NASA se guarda en `eventos.datos` (`jsonb`). El campo
`tipo` usa el código del endpoint (`CME`, `GST`, `SEP`, `FLR`, `IPS`), que
coincide con los valores usados en `tipo_evento_problema`.

## Nota sobre FLR

NASA migró `FLR` a CCMC y la ruta antigua de `api.nasa.gov/DONKI/FLR` devuelve
`HTTP 404`. El cliente usa directamente
`https://ccmc.gsfc.nasa.gov/DONKI-API/get/FLR` para ese endpoint, sin API key.
`CME`, `GST`, `SEP` e `IPS` siguen usando `NASA_API_BASE`.

## Comportamiento de la ventana

`DIAS_VENTANA` no retrasa la ejecución: define cuántos días hacia atrás se
consultan en cada ciclo. Con `2`, cada ciclo pide ayer y hoy. La deduplicación
por `donki_id` evita insertar dos veces el mismo evento cuando se solapan las
consultas.

Si lo que quieres es que el listener se ejecute cada **2 días** (en vez de cada
hora), configura `INTERVALO_MINUTOS=2880`.

## Integración con Mission Trainer

El listener puede enviar las alertas de la IA directamente a la pantalla del juego
**Mission Trainer** conectándose a su servidor relay WebSocket.

- **Juego web**: https://xicotencatl22.github.io/Hacknasa2026-Mission-Trainer-Web/?sala=CODIGO
- **Relay WebSocket**: `wss://mission-trainer-relay.onrender.com`

### Configuración del juego

En tu archivo `.env`:

```env
JUEGO_ENABLED=true
JUEGO_RELAY_URL=wss://mission-trainer-relay.onrender.com
# Con sala: solo esa sala. Sin JUEGO_SALA: MCP global, todas las salas.
JUEGO_SALA=mi-sala-de-prueba
JUEGO_COLOR=#ffcc00
JUEGO_SEGUNDOS=8
```

`JUEGO_SALA` es opcional. El código de sala funciona como contraseña: quien lo
conozca puede controlar el juego. No se imprime en los logs normales. Si no
defines `JUEGO_SALA`, el listener conecta al **MCP global** (`/mcp` sin sala) y el
relay reenvía el mensaje a **todas las salas**; abre el juego con el `?sala=CODIGO`
correspondiente para que reciba el mensaje.

### Probar el envío

1. Abre el juego en el navegador con la misma sala:
   `https://xicotencatl22.github.io/Hacknasa2026-Mission-Trainer-Web/?sala=mi-sala-de-prueba`
2. Envía un mensaje de prueba:

   ```bash
   cargo run -- --juego-test "¡Alerta solar! Radiación entrante."
   ```

   `--juego-test` no necesita `DATABASE_URL` ni `JUEGO_ENABLED`; usa
   `JUEGO_SALA` (si está definida), `JUEGO_COLOR` y `JUEGO_SEGUNDOS`. Sin
   `JUEGO_SALA`, el mensaje va por el MCP global a todas las salas.

3. Para probar la construcción sin conectar al relay:

   ```bash
   cargo run -- --juego-test "Prueba simulada" --dry-run
   ```

### Protocolo del relay

El relay empareja al juego (`/juego?sala=CODIGO`) con uno o varios clientes
MCP: `/mcp?sala=CODIGO` (el mensaje llega solo a esa sala) o `/mcp` sin sala
(MCP global: el relay reenvía el mensaje a **todas** las salas; es el modo que
usa el listener cuando `JUEGO_SALA` no está definida). El listener entra como
cliente MCP y:

- Recibe al conectar `{"tipo":"sistema","juego_conectado":true|false}`.
- Envía `{"id":1,"cmd":"mensaje","args":{"texto":"...","color":"#ffcc00","segundos":8}}`.
- Recibe `{"id":1,"ok":true,"datos":{},"error":"","mensaje":"mostrado"}`.
- Si no hay juego en la sala, el relay responde `{"id":1,"ok":false,"error":"..."}`.

El juego solo muestra `texto`; no tiene campo para `explicacion`, así que esa
parte se guarda en Postgres pero no se envía. El mensaje se trunca a 160
caracteres antes de enviarlo.

El cliente maneja reconexiones con backoff exponencial para absorber el tiempo de
arranque en frío de Render (hasta 1 minuto). Si el relay o la conexión fallan, el
error se registra en consola y nunca interrumpe el guardado en base de datos ni
tumba el proceso.

## Servidor MCP (Claude <-> Juego)

`mcp/servidor_mcp.py` es el servidor MCP que expone las herramientas de
**Mission Trainer** al cliente MCP (Claude Desktop, Claude Code, otro). Es el
puente inverso al listener: mientras el listener habla con el relay con el rol
`mcp` para las alertas de la IA, este servidor se conecta al relay para que
Claude lance eventos, recargue sistemas y ponga mensajes en pantalla.

- **Instalación**: `pip install "mcp[cli]" websockets`.
- **Conexión**: el servidor se conecta a `wss://<relay>/mcp?sala=CODIGO` con
  backoff exponencial (cold starts de Render).
- **Claude Desktop**: `mcpServers -> mission-trainer` con stdio sobre
  `mcp/servidor_mcp.py`.

### Herramientas

| Herramienta | Descripción |
| --- | --- |
| `mostrar_mensaje(texto, color, segundos)` | Muestra un mensaje en la pantalla del juego. |
| `obtener_estado()` | Estado de la conexión MCP con el juego. |
| `recargar_oxigeno(porcentaje)` | Recarga el oxígeno del jugador (1-100). |
| `recargar_combustible(porcentaje)` | Recarga el combustible del jugador (1-100). |
| `leer_eventos()` | Lista los eventos de la NASA pendientes (sin mensaje IA). |
| `lanzar_evento_nasa(tipo, intensidad, fecha, mensaje, dificultad, severidad)` | Lanza un evento de la NASA (`CME`, `GST`, `SEP`, `FLR`, `IPS`). |
| `cancelar_evento_nasa()` | Cancela el evento activo en el juego. |
| `conectar_sala(codigo)` | Conecta el MCP a la sala de un juego abierto en otro dispositivo. |
| `estado_conexion()` | Devuelve si el MCP está conectado y si hay juego en la sala. |

### Configuración del servidor MCP

Variables de entorno del servidor (`mcp/servidor_mcp.py`):

```env
MISSION_RELAY=wss://mission-trainer-relay.onrender.com
MISSION_SALA=mi-sala-1
SUPABASE_URL=https://<proyecto>.supabase.co
SUPABASE_KEY=tu_service_key
```

- `MISSION_RELAY` overridea la URL del relay (p. ej. `wss://tu-servicio.onrender.com`).
- `MISSION_SALA` conecta a esa sala al arrancar (opcional; si no se define, Claude
  usa `conectar_sala`).
- `SUPABASE_URL` / `SUPABASE_KEY` se usan solo para `leer_eventos`.

Desde Claude Desktop:

```json
{
  "mcpServers": {
    "mission-trainer": {
      "command": "python",
      "args": ["...\\...\\Hacknasa2026-Mission-Trainer\\mcp\\servidor_mcp.py"],
      "env": {
        "MISSION_RELAY": "wss://mission-trainer-relay.onrender.com",
        "MISSION_SALA": "nasa123"
      }
    }
  }
}
```

El plan gratuito de Render se duerme tras 15 minutos sin uso; la primera conexión
después de eso puede tardar hasta un minuto en conectarse.

