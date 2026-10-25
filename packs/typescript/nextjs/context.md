Next.js 16 App Router. Request APIs (`params`, `searchParams`, `cookies()`, `headers()`, `draftMode()`) are async: await them.
Components are Server Components unless the file starts with `'use client'`.
Request interception lives in `proxy.ts` (not `middleware.ts`).
`revalidateTag(tag, "max")` takes a cacheLife profile; `updateTag(tag)` in a Server Action gives read-your-writes.
Remote images are allowed with `images.remotePatterns`.
`next lint` no longer exists and `next build` does not lint; lint runs through Biome.
Server Actions and route handlers (`app/**/route.ts`) are public endpoints: validate their input with a schema and check who is calling (not checked by Forge).
`NEXT_PUBLIC_` variables are inlined into the browser bundle; one named like `*SECRET*`, `*PRIVATE*`, `*PASSWORD*`, `*SERVICE_ROLE*` or `*DATABASE_URL*` is rejected.
