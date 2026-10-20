React 19 in the App Router: components are Server Components unless their file starts with `'use client'`.
State, effects, context, refs and event handlers need a Client Component; keep `'use client'` at the leaves.
Server Actions are async functions in a file or function marked `'use server'`.
