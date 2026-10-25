You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

# MCP auth client: 401 challenges are not readable by browser clients across origins

## Problem

`createMcpAuthClient` (from the MCP plugin's client entry point) protects MCP endpoints. When a request has no bearer token, or an invalid or expired one, it responds with `401` and a `WWW-Authenticate` challenge such as:

```
Bearer resource_metadata="https://mcp.example.com/.well-known/oauth-protected-resource"
```

Browser-based MCP clients use this challenge to discover the protected-resource metadata and start OAuth. On cross-origin requests, browsers hide response headers unless they are allowed and exposed through CORS.

Today the 401 responses carry only `WWW-Authenticate`. They do not include the CORS headers the client already uses for other responses, and they do not list `WWW-Authenticate` in `Access-Control-Expose-Headers`. A browser client therefore cannot read the challenge. This affects both the Fetch-style `client.handler(...)` wrapper and the Node/Express-style middleware.

## Required behaviour

Each `401` produced by the MCP auth client must include:

- **`WWW-Authenticate`**, unchanged from today's value.
- **The client's normal CORS headers.** In particular, `Access-Control-Allow-Origin` must equal the origin of the configured `authURL` (for example, `https://auth.example.com`).
- **`Access-Control-Expose-Headers`** that includes `WWW-Authenticate`.

This applies in all of these cases:

1. `client.handler(fn)` when the `Authorization` header is missing or is not a `Bearer` token.
2. `client.handler(fn)` when the bearer token fails verification.
3. The Node-style middleware for both failure cases. This covers responses that use `res.set(...)` with `res.status(...).json(...)` (Express-like) and responses that use `res.writeHead(...)` with `res.end(...)`.

### Merging `Access-Control-Expose-Headers`

The exact value of `Access-Control-Expose-Headers` is checked, so follow these rules:

- **Nothing exposed yet:** the value must be exactly `WWW-Authenticate`.
- **Header already set on a Node-style response:** append to the existing value as `<existing>, WWW-Authenticate`. Read the existing value via `res.get(name)` when available; otherwise use `res.getHeader(name)`, which may return a string, number, or string array. Join an array with `", "`. For example, if the response already has `Access-Control-Expose-Headers: set-auth-token`, the result must be `set-auth-token, WWW-Authenticate`.
- **Already listed:** if `WWW-Authenticate` already appears in the list (case-insensitive, ignoring surrounding whitespace), leave the value unchanged and do not duplicate it.

### Node-style responses

For Express-like responses, every resulting header (CORS headers, expose headers, and `WWW-Authenticate`) must be applied through `res.set(name, value)` before the status and JSON body are sent.

The accepted response shape may optionally provide:

- `get(name): string | undefined`
- `getHeader(name): string | number | string[] | undefined`

Both are optional. Responses that lack them must still work.

The JSON-RPC error bodies and messages must not change.
