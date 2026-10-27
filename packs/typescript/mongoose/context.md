Mongoose: request input (`req.body`, `req.query`, `req.params`) is validated or passed through `sanitizeFilter()` before it reaches a filter or update; passing it whole is rejected (operator injection).
No `$where` (server-side JavaScript); use query operators or `$expr`.
