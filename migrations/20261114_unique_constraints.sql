-- Requerido por los INSERT ... ON CONFLICT del listener.
-- Si ya hay duplicados, bórralos antes de aplicar los índices.
create unique index if not exists mensajes_ia_evento_id_key
    on public.mensajes_ia (evento_id);

create unique index if not exists sync_nasa_endpoint_key
    on public.sync_nasa (endpoint);
