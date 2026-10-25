Hono: request data is validated by middleware on the route, `zValidator("json" | "query" | "form" | "param", schema)` from `@hono/zod-validator` or `validator()` from `hono/validator`, and read back with `c.req.valid("json")` etc., which is typed.
In files that import from `hono` or `@hono/*`, `c.req.json()`, `c.req.parseBody()`, `c.req.formData()`, `c.req.query()` and `c.req.queries()` are rejected; `c.req.param()` and headers are not checked.
`cors()` with `credentials: true` names its origins; `origin: "*"` or `(origin) => origin` with credentials is rejected.
