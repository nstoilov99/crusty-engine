# CLAUDE.md — crusty-gui

In-house immediate-mode GUI **library** (egui replacement) for the Crusty engine.
This repo is the library + demo, not the editor — panels, undo/redo, play mode,
ECS and the asset browser live in `../rust-engine`.

Pure 2D screen-space UI: y-down, `pixels_per_point` 1.0. The engine's Z-up
convention does not apply here.

## Source of truth
`ROADMAP.md` — phases, what's done, what's next. Current: **Phase 16** — the
rust-engine editor migrates from egui to crusty-gui panel by panel (Console
first), with egui kept as fallback until parity.

## Module map
- `src/context` — `Context`, `Ui`, frame lifecycle (`begin_frame` → `run_root` → `end_frame` → `FrameOutput`)
- `src/paint` — `PaintCmd`, tessellator, meshes, texture types (`TextureId`, `TextureFilter`)
- `src/text` — cosmic-text shaping + glyph atlas (`TextRenderer`)
- `src/widgets` — built-in widgets (label, button, checkbox, slider, text_edit, combo_box, ...)
- `src/style` — theme tokens, palettes, spacing, fonts
- `src/dock` — dock tree, splits, tabs
- `src/layer` — z-order for floating UI
- `src/backend` — vulkano renderers; **`TargetRenderer` is the embedding seam**
  (borrows the host-owned `TextRenderer` per call, returns a command buffer, host presents)
- `src/shell` — winit driver (`Shell`), multi-window, input translation (`shell::input`), `winit_cursor`
- `src/window` — winit/vulkano window + swapchain layer
- `src/main.rs` — demo binary

## Features
`winit-window`, `vulkano-renderer`; `shell` requires both. Default enables all.

## Consumer: rust-engine
`../rust-engine` depends on this via path dep behind its `crusty` feature
(implies `editor`). Engine-side seam: `engine/src/engine/gui/crusty.rs`
(`CrustyGui` on the main thread, `CrustyRenderer` on the render thread,
`SharedTextRenderer` mutex between them; paint list crosses in `FramePacket`).

Run the editor with crusty enabled:
```bash
cd ../rust-engine && cargo run -p game_client --features editor,crusty
```

## Verification
```bash
cargo test
cargo clippy --all-features --all-targets   # don't add new warnings
cargo run                                    # demo — manual smoke test
cd ../rust-engine && cargo check -p rust_engine --features editor,crusty  # don't break the consumer
```

## Conventions
- Commit messages: short, imperative. Never add Co-Authored-By.
- `TargetRenderer` sync contract: its command buffers must execute serially
  (glyph atlas and blur targets are shared across frames).
- Keep widget code readable; extract helpers when nesting hurts.
- API changes to `backend`/`shell` must be checked against rust-engine before commit.
