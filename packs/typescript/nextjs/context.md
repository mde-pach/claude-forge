Next.js 16 App Router. Request APIs (`params`, `searchParams`, `cookies()`, `headers()`, `draftMode()`) are async: await them.
Components are Server Components unless the file starts with `'use client'`.
Request interception lives in `proxy.ts` (not `middleware.ts`).
`revalidateTag(tag, "max")` takes a cacheLife profile; `updateTag(tag)` in a Server Action gives read-your-writes.
Remote images are allowed with `images.remotePatterns`.
`next lint` no longer exists and `next build` does not lint; lint runs through Biome.
Server Actions are public POST endpoints: validate their input with Zod and check authorization inside each one.
Variables prefixed `NEXT_PUBLIC_` are inlined into the browser bundle; secrets never use that prefix.
