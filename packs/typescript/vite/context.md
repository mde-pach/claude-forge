`VITE_` variables are inlined into the client bundle: secrets never get the prefix (names with SECRET, PRIVATE, PASSWORD, SERVICE_ROLE or DATABASE_URL are rejected in code and `.env` files).
`define` in `vite.config.*` never inlines the whole `process.env`, and `server.allowedHosts` / `preview.allowedHosts` list hosts instead of `true`.
