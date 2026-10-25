This project is an npm library: its public API is what the entry points in `package.json` `exports` (and `main`, `module`, `types`) export.
A new export of a module that an entry point re-exports by name (`export { a, b } from "./m"`) is added to that list, or is not exported.
An exported name is not removed or renamed outright: it stays exported as a `/** @deprecated Use <new name> */` alias until a major version; removing one without that is rejected at Stop.
