Nuxt: server routes in `server/api/` and `server/routes/` read input with h3's validated helpers, `readValidatedBody(event, schema.parse)`, `getValidatedQuery(event, schema.parse)`, `getValidatedRouterParams(event, schema.parse)`; `readBody`, `getQuery` and `getRouterParams` under `server/` are rejected.
`runtimeConfig.public` is sent to the browser: keys naming a secret, private key, password, service role or database URL there are rejected; secrets stay at the top level of `runtimeConfig`.
`vue-tsc` checks types once `nuxt prepare` has written `.nuxt/`.
