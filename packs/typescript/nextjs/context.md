Next.js 16 App Router. Request APIs (`params`, `searchParams`, `cookies()`, `headers()`, `draftMode()`) are async: await them.
Components are Server Components unless the file starts with `'use client'`.
Request interception lives in `proxy.ts` (not `middleware.ts`).
`revalidateTag(tag, "max")` takes a cacheLife profile; `updateTag(tag)` in a Server Action gives read-your-writes.
Remote images are allowed with `images.remotePatterns`.
`next lint` no longer exists and `next build` does not lint; lint runs through Biome.
Server Actions are public POST endpoints. An exported async function in a `'use server'` file that takes input calls a schema's `.parse()` / `.safeParse()` and an auth helper (`auth()`, `getSession()`, `currentUser()`, ...); both are checked.
Route handlers (`app/**/route.ts`) are public too: one that reads `req.json()`, `req.formData()` or `searchParams` parses it with a schema in the handler (checked); a POST, PUT, PATCH or DELETE handler without an auth helper, auth wrapper (`withAuth(...)`) or signature check gets advice.
`NEXT_PUBLIC_` variables are inlined into the browser bundle; one named like `*SECRET*`, `*PRIVATE*`, `*PASSWORD*`, `*SERVICE_ROLE*` or `*DATABASE_URL*` is rejected.
