Tailwind CSS 4: configuration lives in CSS (`@import "tailwindcss"`, `@theme`); a `tailwind.config.*` is read only when a CSS file loads it with `@config`.
Utilities removed in v4 generate nothing and are rejected: `bg-opacity-*` and the other `*-opacity-*` (use `bg-black/50`), `flex-shrink-*` / `flex-grow-*` (`shrink-*` / `grow-*`), `overflow-ellipsis` (`text-ellipsis`), `decoration-slice` / `decoration-clone` (`box-decoration-*`).
A `<style>` block in a `.vue`, `.svelte` or `.astro` file, or a CSS module, that uses `@apply` starts with `@reference "<path to the main CSS>"`.
