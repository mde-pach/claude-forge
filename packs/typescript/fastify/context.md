Fastify 5: a plugin is `async (instance, opts)` or `(instance, opts, done)`, never async with `done`.
Route schemas (`body`, `querystring`, `params`, `headers`, `response.<code>`) are full JSON Schemas (`{ type: "object", properties: … }`) or a schema builder's output; the v4 shorthand property map is removed.
