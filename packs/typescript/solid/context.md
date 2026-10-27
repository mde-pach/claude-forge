SolidJS: a component runs once; props are reactive getters, so they are read as `props.x` (or split with `splitProps`), never destructured.
Lists render with `<For each={...}>` (or `<Index>`), not `.map()` in JSX.
