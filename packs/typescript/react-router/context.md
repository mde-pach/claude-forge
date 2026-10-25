React Router framework mode: route modules export `loader` (GET data) and `action` (mutations), typed with `Route.LoaderArgs` / `Route.ActionArgs` from `./+types/<route>`.
An exported `action` under `app/` that reads `request.formData()` or `request.json()` validates it with a schema (`.parse` / `.safeParse`, Conform's `parseWithZod`, ...); one without is rejected.
An exported `action` under `app/` checks who is calling (`requireUser`, `requireUserId`, `getSession`, `auth`, ...); one without is rejected. Public actions use a local rule of the same id.
Responses use `data()` and `redirect()`; `json()` and `defer()` are gone.
