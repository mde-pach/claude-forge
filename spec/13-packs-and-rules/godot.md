<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `godot`

### godot

Godot Engine 4 projects (GDScript, scenes and resources).

Detected by: a file matching `**/project.godot`.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `godot/headless-measure` | Measuring gameplay values (jump reach, speed, timings) by running the game's real scenes in a headless Godot script. | on-demand | The task designs or changes a level's layout, gaps, platforms or distances.<br>The task tunes or checks movement, physics or timing values of a game. | wherever its pack is |
| `godot/reimport` | Rescanning a Godot project headless after moving, renaming or adding files outside the editor, so its UID and class caches follow. | on-demand | The task moves or renames files or folders of a Godot project.<br>The task adds or renames a GDScript class_name. | wherever its pack is |
| `godot/render-bench` | Finding which rendering feature costs a Godot scene its frame time, by switching features one at a time and measuring. | on-demand | The task is to improve a game's frame rate or rendering performance. | wherever its pack is |
| `godot/scene-capture` | Capturing screenshots of a Godot scene at several window sizes, or from the same views before and after a change, to compare them. | on-demand | The task changes a game's user interface layout, menus or HUD.<br>The task changes a scene's lighting, environment, materials or other visual settings. | wherever its pack is |
