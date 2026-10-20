React components in Astro render to static HTML; they become interactive only with a `client:*` directive (`client:load`, `client:idle`, `client:visible`, `client:only="react"`).
Hooks and event handlers run only in hydrated islands.
Props passed to a hydrated island must be serializable.
