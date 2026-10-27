Prisma 7. Generator `prisma-client` with an explicit `output`; the client is imported from that path.
SQL databases connect through a driver adapter (e.g. `@prisma/adapter-pg`) passed to `new PrismaClient({ adapter })`.
CLI configuration and the connection URL live in `prisma.config.ts`; `.env` is not loaded automatically.
`prisma migrate dev` does not seed: run `prisma db seed`. Query hooks use Client Extensions (`$extends`; `$use` is removed).
Raw SQL uses the tagged `$queryRaw` / `$executeRaw` forms; the `Unsafe` variants, and `Prisma.raw()` built from a template or concatenation, are rejected.
`$transaction(async (tx) => …)` callbacks make no network calls (SDKs included): call them before or after the transaction; `fetch` and calls through an imported axios, got, ky, ofetch, node-fetch or undici there are rejected.
A schema change comes with a migration (`prisma migrate dev`), except in a db push workflow (no `prisma/migrations`, a `prisma db push` script), where none is required.
A new migration that drops a column or table is rejected unless the user accepts that line in `forge.yaml`: drop only after a deploy in which no code reads it (expand and contract).
