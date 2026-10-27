Vue 3 with `<script setup lang="ts">` single-file components; `vue-tsc` type-checks `.vue` files (tsc does not).
Templates are valid as Biome checks them: an element with `v-for` binds a `:key` that uses its item, no `v-if` with `v-for` on one element, directives have their value.
Refs are read with `.value` in script code (never as a condition or operand), and `setup()` does not destructure its props (`toRefs(props)` instead).
`v-html` and `:innerHTML` in `.vue` files are rejected: they render unescaped HTML. Render text with `{{ }}`, or sanitise in one dedicated component.
Vue 2 template syntax is rejected (Vue 3 ignores it): `v-model:prop` instead of `.sync`, `v-slot` / `#name` instead of `slot-scope`, no `.native`, key names instead of key codes.
