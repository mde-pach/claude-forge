Vue 3 with `<script setup lang="ts">` single-file components; `vue-tsc` type-checks `.vue` files (tsc does not).
An element with `v-for` binds a stable `:key` (an id, not the index); one without `:key` is rejected (`<template v-for>` is not checked).
`v-html` and `:innerHTML` in `.vue` files are rejected: they render unescaped HTML. Render text with `{{ }}`, or sanitise in one dedicated component.
eslint-plugin-vue (`vue/require-v-for-key`, `vue/no-v-html`, …) is the ecosystem's linter; Forge does not run or audit ESLint.
Vue 2 template syntax is rejected (Vue 3 ignores it): `v-model:prop` instead of `.sync`, `v-slot` / `#name` instead of `slot-scope`, no `.native`, key names instead of key codes.
