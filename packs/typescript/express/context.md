Express sessions: `express-session` cookie options and `cookie-session` options never set `secure: false` or `httpOnly: false` literally (an environment-dependent value is fine).
Behind a proxy, `trust proxy` (Express) is a hop count, the proxy subnets or a function, not `true` (advice).
Express 5 (path-to-regexp 8): a wildcard is named (`/*splat`, `/{*splat}`), an optional segment is `{...}`; `?`, `+`, `(…)` in paths are rejected.
Express 5: `req.query` is a getter (keep parsed input in `res.locals`); async handlers need no wrapper (`express-async-errors` is rejected); `res.sendStatus(n)`, not `res.send(n)`; no `res.redirect("back")`.
