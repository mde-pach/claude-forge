# Forge mascot

One source, two outputs.

| File | What it is |
| --- | --- |
| `mascot.frames` | The frames. Edit this. |
| `build.py` | Regenerates every output below. Run `python3 docs/mascot/build.py`. |
| `forge-intro.sh` | CLI animation, plain POSIX `sh`. |
| `../src/components/forge-mascot.js` | Website component `<forge-mascot>`, no dependencies. |
| `../public/logo.svg`, `../public/favicon.svg` | Static logo (site header) and favicon, from the final frame's pixels. |
| `forge-logo.txt` | The final frame as plain text (README, `--version` output). |

## Editing frames

Frames are drawn in pixels: `#` filled, `.` empty. Every 2 pixel rows become 1 terminal line of half-block characters. Each frame starts with `@ <seconds>`, the last one is the logo, and `@ blink` is the logo with eyes closed, used for blinking. All frames must be the same height; `build.py` stops with an error if not.

## CLI

```sh
sh forge-intro.sh                 # intro, blinks for 3 s, logo stays on screen
FORGE_HOLD=0 sh forge-intro.sh    # no blinking after the intro
FORGE_HOLD=-1 sh forge-intro.sh   # blink until Ctrl-C
FORGE_COLOR=33 sh forge-intro.sh  # optional ANSI color
FORGE_STATIC=1 sh forge-intro.sh  # logo only, no animation
```

Output that is not a terminal (piped, CI logs) gets the static logo automatically, so it never prints escape codes into logs.

## Website (Starlight / Astro)

`build.py` writes `forge-mascot.js` straight into `docs/src/components/`. In any `.astro` or `.mdx` page:

```astro
<script>import "../components/forge-mascot.js";</script>
<forge-mascot></forge-mascot>
```

Each mascot blinks at random intervals after the intro. Add `no-blink` to turn that off.

The mascot uses the surrounding text color, so it follows Starlight's light and dark themes. Size it with `font-size`:

```css
forge-mascot { font-size: 2rem; color: var(--sl-color-accent); }
```

Attributes: `static` shows only the logo, `loop` replays continuously, `label="..."` sets the accessible name. Clicking it replays the animation, and visitors with reduced motion enabled see the logo without animation.
