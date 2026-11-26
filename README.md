# Crusty Engine

A 3D game engine in Rust: Vulkan (Vulkano 0.35) deferred renderer, custom ECS
over hecs, Rapier physics, node-graph authoring (animation graphs, visual
scripting), an in-house editor UI (`crusty-gui`), and server-authoritative
multiplayer on SpacetimeDB.

**Coordinate system: Z-up, X-forward.** Game logic is Z-up everywhere; the
`render_adapter` module converts to Vulkan's Y-up at render time. Details and
gotchas: `docs/KNOWLEDGE.md` ▸ Coordinate System.

## Build and run

Prerequisites: stable Rust, the Vulkan SDK (validation layers optional), a
Vulkan 1.2 GPU. Linux additionally needs the packages listed in
`.github/workflows/ci.yml` ▸ Linux job.

```bash
cargo run -p game_client --features editor                           # editor
cargo run -p game_client                                             # standalone game
cargo run -p game_client -- --scene scenes/locomotion_demo.scene     # standalone, offline scene
cargo run -p game_client -- --import-anim <src.fbx> <out.anim> --import-scale 0.01
cargo run -p game_client --release -- --benchmark                    # benchmark runner (docs/FLAGS.md)
```

Full local CI gate (what GitHub Actions runs): `pwsh scripts/ci_local.ps1`
or `bash scripts/ci_local.sh`; `-Quick` / `--quick` for check + clippy + fmt.

Multiplayer: `scripts/export_windows.ps1` then `scripts/host_local.ps1`
(local SpacetimeDB + exported client). See `docs/roadmap/M9.5-COOP-RUNBOOK.md`.

## Workspace

| Crate / dir | Role |
|---|---|
| `engine/` | The engine library: rendering, ECS, editor, physics, animation, assets, plugins |
| `game_shared/` | Engine-free gameplay rules shared with the server (wasm32-checked) |
| `game_client/` | The binary: editor (`--features editor`) or standalone client |
| `game_client_net/` | SpacetimeDB client bindings and net acceptance tests |
| `server/game_module/` | SpacetimeDB module (built by the `spacetime` CLI, own lockfile) |
| `crates/node_graph_{types,exec,macros}`, `crates/curve_asset` | Portable node-graph crates (CI-checked standalone and for wasm32) |
| `crates/transform-gizmo-fork`, `crates/emath-patched` | Patched dependencies (Z-up gizmo) |
| `tools/` | `pak_tool`, `collision_cooker`, `greybox_gen`, `net_bots`, `migrate_asset_extensions` |
| `content/` | Scenes, graphs, animations, meshes, materials, cooked collision, world cells |
| `scripts/` | Export, local hosting, smoke/soak, design lint, local CI, screenshots |

## Documentation

| File | Contents |
|---|---|
| `CLAUDE.md` | Working context for agents and contributors: conventions, threading model, run commands |
| `docs/ARCHITECTURE.md` | System architecture and module relationships |
| `docs/KNOWLEDGE.md` | Conventions, patterns and gotchas per subsystem |
| `docs/DECISIONS.md`, `docs/adr/` | Architectural decision records |
| `docs/PLUGINS.md` | Plugin author guide |
| `docs/FLAGS.md` | Runtime CLI flags |
| `docs/roadmap/ROADMAP.md` | Where the engine is, open follow-ups, remaining tasks |
| `docs/roadmap/COMPLETED.md` | Finished tasks with close-outs and deferred ledgers |

## Status

See `docs/roadmap/ROADMAP.md` ▸ Where we are. The networked co-op slice
milestone is achieved on packaged builds; current work is Phase 11
(visual scripting tail, node graph editor v2).
