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

## Code style
- Always strive for concise, simple solutions.
- If a problem can be solved in a simpler way, propose it.

## General preferences
- If asked to do too much work at once, stop and state that clearly.
- If computer use is helpful for completing or verifying work (screenshots,
  visual comparison, driving a running app), shell out to gpt-5.6-Sol via Codex
  (capture with `.claude/skills/port-panel/screenshot.ps1`, compare with
  `codex exec -i`).

## Picking the right model for workflows and subagents
Rankings, higher = better. Cost reflects what the user actually pays (OpenAI is
near-free due to a deal), not list price. Intelligence is how hard a problem
you can hand the model unsupervised. Taste covers UI/UX, code quality, API
design and copy.

| model       | cost |  intelligence   |  taste |
|-------------|------|-----------------|--------|
| gpt-5.6-Sol | 9    |  8              | 6      |
| sonnet-5    | 5    |  5              | 7      |
| opus-4.8    | 4    |  7              | 8      |
| fable-5     | 2    |  9              | 9      |

How to apply:
- These are defaults, not limits. You have standing permission to override them: if a cheaper model's output doesn't meet the bar, rerun or redo the work with a smarter model without asking. Judge the output, not the price tag. Escalating costs less than shipping midiocre work.
- Cost is a tie-breaker only; when axes confict for anything that ships, intelligence > taste > cost.
- Bulk/mechanical work (clear-spec implementation, data analysis, migrations): gpt-5.6-Sol it's effectively free.
- Reviews of plans/implementation: fable-5 or opus-4.8, optionally gpt-5.6-Sol as an extra independent perspective.
- Never use Haiku.

## Visual Changes and Computer Use
- When implementing/fixing visual change shell out gpt-5.6-Sol to take a screenshot of the change and have fable-5 and gpt-5.6-Sol discuss it.
- Have fable-5 review the result for UI/UX quality, visual hierarchy, consistency, and overall taste.
- Reconcile the findings and fix material issues before considering the task complete.

## Following Visual References
- When following a visual reference, aim for approximately 90–95% visual similarity unless pixel-perfect reproduction is explicitly requested.
- Prioritize:
   - Correct functionality and usability
   - Consistency with the product's existing visual language
   - Good UX and accessibility
   - The important composition, hierarchy, spacing, typography, and visual cues from the reference
   - Minor decorative similarity
- Do not reproduce defects or poor UX from a reference merely for visual fidelity.
- When intentionally deviating from the reference, preserve its core intent and briefly explain any material UX or consistency improvements.

## Conventions
- Commit messages: short, imperative. Never add Co-Authored-By.
- `TargetRenderer` sync contract: its command buffers must execute serially
  (glyph atlas and blur targets are shared across frames).
- Keep widget code readable; extract helpers when nesting hurts.
- API changes to `backend`/`shell` must be checked against rust-engine before commit.
