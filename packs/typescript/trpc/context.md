tRPC: a procedure that takes input declares `.input(<schema>)` (Zod or another validator) and reads it from `input`; `.input(z.any())`, `.input(z.unknown())` and a cast-only function are rejected.
Errors thrown in a procedure are `TRPCError` with a `code` (`NOT_FOUND`, `FORBIDDEN`, `BAD_REQUEST`, …); a plain `Error` is sent as INTERNAL_SERVER_ERROR and is rejected.
