# Rust Game Engine - AI Context

## Quick Reference

| Aspect | Value |
|--------|-------|
| **Coordinate System** | Z-up (X=Forward/Red, Y=Right/Green, Z=Up/Blue) |
| **Renderer** | Vulkano 0.35, Deferred Pipeline |
| **ECS** | Custom (wrapping hecs 0.10) |
| **GUI** | In-house `crusty-gui` (workspace crate `crates/crusty-gui`) — egui fully removed |
| **Physics** | Rapier 3D 0.25 |
| **Serialization** | RON (Rusty Object Notation) |

## Critical: Coordinate System

```
Game World (Z-up)          Vulkan Render (Y-up)
      Z (Up/Blue)                Y (Up)
      |                          |
      |                          |
      +------ Y (Right/Green)    +------ X (Right)
     /                          /
    X (Forward/Red)           -Z (Forward)
```

**Conversion**: All game logic uses Z-up. The `render_adapter` module converts to Y-up at render time.

- Use `local_matrix_zup()` for hierarchy composition
- Use `model_matrix()` or `world_matrix_to_render()` for rendering
- Gizmo colors: X=Red, Y=Green, Z=Blue

## Before Making Changes

1. **Read relevant docs** in `docs/` folder for the subsystem you're modifying
2. **Check existing patterns** in similar files
3. **Maintain Z-up convention** - never mix coordinate systems
4. **Run the local gate** before considering work complete: `pwsh scripts/ci_local.ps1 -Quick` (or `bash scripts/ci_local.sh --quick`) — fmt + clippy `-D warnings` on both feature sets; the full script mirrors CI

## Key Documentation

| File | Contents |
|------|----------|
| `docs/ARCHITECTURE.md` | System architecture and module relationships |
| `docs/KNOWLEDGE.md` | Conventions, patterns, and gotchas |
| `docs/DECISIONS.md` | Architectural decision records |
| `docs/roadmap/ROADMAP.md` | Goal, status board, how work runs, milestones M11–M20 (finished work: `COMPLETED.md`) |
| `docs/EDITOR-UX.md` | Which editor each surface follows, and the pattern to copy |
| `CONTEXT.md` | Domain glossary — canonical terms for graphs and animation |

## Code Rules

### Do
- Use `Result<T, E>` for error handling
- Use `profile_function!()` / `profile_scope!()` for performance-critical code
- Components are plain data structs (no behavior)
- Systems are stateless functions
- Use builder patterns for complex struct construction

### Don't
- Use `unwrap()` in production code (use `?` or handle errors)
- Use `Box<dyn Component>` in ECS (type-safe storage only)
- Mix coordinate systems (Z-up for logic, Y-up only for Vulkan)
- Add behavior to components (put logic in systems)

## Module Overview

```
src/engine/
├── core/        # Vulkan context, device, swapchain
├── rendering/   # 2D/3D pipelines, deferred renderer
├── ecs/         # Entity-Component-System (hecs wrapper)
├── editor/      # Editor panels, viewport, gizmos
├── physics/     # Rapier 3D integration
├── assets/      # Asset loading and management
├── gui/         # crusty-gui integration (main-thread layout / render-thread seam)
├── adapters/    # Coordinate conversion (Z-up ↔ Y-up)
└── camera/      # Camera systems
```

## Features and run commands

```bash
cargo run -p game_client --features editor   # editor (crusty-gui)
cargo run -p game_client                     # standalone game
cargo run -p game_client -- --scene scenes/locomotion_demo.scene   # standalone, offline scene
cargo run -p game_client -- --import-anim <src.fbx> <out.anim> --import-scale 0.01   # animation-only import (either build)
```

`editor` pulls the workspace crate `crates/crusty-gui`. `crusty` is a deprecated alias for `editor`. `hud` (default)
is crusty-gui without editor machinery; `graph-scripting` (default) gates the
interpreter; `dev_nodes` registers the demo node set.

## Threading model (rendering)

Main thread does game logic, ECS and UI layout (CPU-only); the render thread
records command buffers and presents. The only data crossing is `FramePacket`
(bounded(2) crossbeam channel) — all fields owned, no shared mutable state,
except the crusty `SharedTextRenderer` mutex (layout shapes text on main,
renderer flushes glyph uploads on record).

## GUI (crusty-gui)

The editor UI is the in-house `crusty-gui` crate (Phase 16 migration complete —
egui is fully removed). Seam: `engine/src/engine/gui/crusty.rs` (`CrustyGui` main thread /
`CrustyRenderer` render thread, `SharedTextRenderer` mutex between them; paint
list crosses in `FramePacket`). Panels are thin drawing layers — logic stays in
state structs and systems. Dock layout persists to `editor_layout_crusty.ron`.

Note: no logger is installed in `game_client` — `log::` output is invisible;
use `println!` for temporary diagnostics (and remove before commit).

## Current Development Focus

What is next, claimed and done lives in `docs/roadmap/ROADMAP.md` ▸ Status
board (nothing here repeats it); finished tasks (with deferred ledgers) in
`docs/roadmap/COMPLETED.md`.

- Goal: one engine for an MMO with a large seamless world and for large
  offline open-world games (`ROADMAP.md` ▸ Goal).
- Networked Co-op Slice milestone (M0–M10) and Tasks 40–41.7, 45-A:
  **complete**; 41.7's live check by the user is pending.

## Agent workflow

- **Claim → plan → implement → close out** a roadmap task with the
  `roadmap-task` skill; it names every file a close-out must touch. There is
  no plan file per task: the plan goes in the PR description.
- **Tracks:** stay inside your track's paths (`ROADMAP.md` ▸ Tracks); shared
  code changes through a small PR that merges first.
- **Repo-specific recipes** (model-invoked skills in `.claude/skills/`, mirrored
  for Codex in `.agents/skills/`): `add-node`, `add-component`,
  `add-render-pass`, `add-editor-panel`, `editor-ux`, `run-app`. Process skills (`tdd`,
  `code-review`, `diagnosing-bugs`, …) are the installed Matt Pocock set.
- **Working area:** `.scratch/` (gitignored) for tickets, review transcripts,
  recordings.
- **Branches / PRs:** one branch per task (`m<NN>-<task>-<slug>`), PR to `main`;
  conventional commit subjects, short and imperative, no `Co-Authored-By`
  (`.githooks/prepare-commit-msg` strips them — enable with
  `git config core.hooksPath .githooks`).
- **PR lifecycle skills** (Theo's `t3dotgg/tstack`): `file-pr`, `babysit-pr`,
  `sol-loop`, `council`, `full-send` / `full-stop`, `takeover`, `modernize`,
  `leaving-pr-comments`. Council members for this repo: Claude Opus 5.5,
  GPT Sol (latest), Claude Fable 5.2; `sol-loop` reviews with GPT Sol.
  `full-send` may merge — only on an explicit "full send", which covers that
  one PR.
- **Merging.** The rules are in `ROADMAP.md` ▸ Merge policy and PR evidence,
  and only there. Agents never edit the next line.

  Merge policy: owner

- **Research** that spans many files goes to Codex (read-only prompt); see
  *Picking the right model for workflows and subagents* below.

## Patched Dependencies

These crates are forked/patched in `crates/`:
- `transform-gizmo` - Modified for Z-up coordinate system
- `emath` - only remains as a dependency of the `transform-gizmo` fork (egui itself is gone)

## Code style
- Always strive for concise, simple solutions.
- If a problem can be solved in a simpler way, propose it.

## General preferences
- If asked to do too much work at once, stop and state that clearly.
- If computer use is helpful for completing or verifying work (screenshots,
  visual comparison, driving a running app), shell out to gpt-5.6-Sol via Codex
  (capture with `scripts/screenshot.ps1`,
  compare with `codex exec -i`). Codex is configured via `.codex/config.toml`
  (model gpt-5.6-sol, workspace-write sandbox).

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
- These are defaults, not limits. You have standing permission to override them: if a cheaper model's output doesn't meet the bar, rerun or redo the work with a smarter model without asking. Judge the output, not the price tag. Escalating costs less than shipping mediocre work.
- Cost is a tie-breaker only; when axes conflict for anything that ships, intelligence > taste > cost.
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
  (glyph atlas and blur targets are shared across frames). `CrustyRenderer`
  upholds this on the render thread.
- Keep panel code readable; extract helpers when nesting hurts.
- Changes that need new crusty-gui `backend`/`shell` API are made in
  `crates/crusty-gui` in the same commit.

## PR Guidelines
- Do not include HTML comments (e.g. <!-- generated-by-cyrus -->)
- Do not include the "Tip" block about @mentioning cyrusagent
- Do not append a "Linear issue:" link at the bottom
- Do not add "Generated with Claude Code" or any other AI attribution line to
  PR descriptions or commits (`.claude/settings.json` turns it off for Claude
  Code; `.github/workflows/pr-body-clean.yml` strips it from PR descriptions)