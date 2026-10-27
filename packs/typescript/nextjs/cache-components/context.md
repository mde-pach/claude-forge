Next.js runs with `cacheComponents: true`. Data is cached with the `'use cache'` directive plus `cacheLife()` and `cacheTag()` from `next/cache`, not with `unstable_cache`.
Route segment config exports (`dynamic`, `revalidate`, `fetchCache`, `experimental_ppr`) fail `next build` in this mode: use `'use cache'` and `cacheLife()` instead, and `<Suspense>` around what reads request data.
