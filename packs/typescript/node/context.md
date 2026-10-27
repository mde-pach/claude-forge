TypeScript files run on Node with `node file.ts` (type stripping) or `tsx`; `ts-node` is unmaintained and rejected in package.json, and running it asks.
`node:vm` is not a security boundary: never use it to run untrusted code.
