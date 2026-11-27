# Agent instructions

Read `CLAUDE.md` first. It is the authoritative context for this repository
(coordinate system, threading model, features, code rules). This file only adds
what Codex-specific tooling looks for.

## Merging

The rules live in `docs/roadmap/ROADMAP.md` ▸ Merge policy and PR evidence;
the `Merge policy` line itself is in `CLAUDE.md`. Never edit that line.
No merge on red, and never `--admin`.

## Code Review Rules

Codex code review reads this section. Report only findings a maintainer would
act on; prefix nothing, add no praise, one finding per comment.

### Coordinate system
- Game logic is Z-up (X forward, Y right, Z up). Only the `render_adapter` path
  converts to Vulkan Y-up. Flag any math that mixes the two or converts outside
  that module.
- Hierarchy composition must go through `local_matrix_zup()`; rendering through
  `model_matrix()` or `world_matrix_to_render()`.

### Correctness
- No `unwrap()` or `expect()` on fallible paths in non-test code; use `?` or
  handle the error.
- ECS components are plain data; systems are stateless functions. Flag behaviour
  added to a component or state smuggled into a system.
- Anything crossing the render-thread boundary must respect the threading model
  in `CLAUDE.md`; flag locks held across frame submission and shared mutable
  state without a documented owner.
- Editor code: undo/redo must record every mutation it exposes, and play/edit
  mode transitions must not leak runtime state back into the edited scene.
- Tests that were loosened, deleted or marked ignored to make a change pass.

### Scope and dependencies
- New dependencies need a one-line justification in the PR; prefer what the
  workspace already uses (`glam`, `serde`, `rayon`, `vulkano`, `winit`).
- Path dependencies must point inside this workspace (`crates/...`), never to a
  sibling checkout.

### Do not report
- Formatting, import order, or anything `cargo fmt` / `cargo clippy` enforces.
- Style preferences with no functional impact.
- Issues in vendored code under `transform-gizmo-fork/`.
