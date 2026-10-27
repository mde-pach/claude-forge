Mongoose: request input (`req.body`, `req.query`, `req.params`) is validated or passed through `sanitizeFilter()` before it reaches a filter or update; passing it whole is rejected (operator injection).
No `$where` (server-side JavaScript); use query operators or `$expr`.
Mongoose 9: an update pipeline (an array) needs `{ updatePipeline: true }` on the call or `mongoose.set("updatePipeline", true)`.
