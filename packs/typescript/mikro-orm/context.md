MikroORM: each request works in its own context (`RequestContext` middleware or `em.fork()`); `allowGlobalContext: true` is rejected outside tests.
Schema changes go through migrations (`migration:create`, `migration:up`); `schema:drop --run` / `schema:fresh --run` are refused, `schema:update --run` and `migration:fresh` need confirmation.
