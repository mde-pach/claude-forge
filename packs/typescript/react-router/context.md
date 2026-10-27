React Router framework mode: route modules export `loader` (GET data) and `action` (mutations), typed with `Route.LoaderArgs` / `Route.ActionArgs` from `./+types/<route>`.
An `action` is a public POST endpoint: validate what it reads from `request.formData()` or `request.json()` with a schema and check who is calling (not checked by Forge).
Responses use `data()` and `redirect()`; `json()` and `defer()` are gone.
