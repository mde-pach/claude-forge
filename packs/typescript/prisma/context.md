Prisma 7. Generator `prisma-client` with an explicit `output`; the client is imported from that path.
SQL databases connect through a driver adapter (e.g. `@prisma/adapter-pg`) passed to `new PrismaClient({ adapter })`.
CLI configuration and the connection URL live in `prisma.config.ts`; `.env` is not loaded automatically.
`prisma migrate dev` does not seed: run `prisma db seed`. Query hooks use Client Extensions (`$extends`; `$use` is removed).
Raw SQL uses the tagged `$queryRaw` / `$executeRaw` forms; the `Unsafe` variants are rejected.
