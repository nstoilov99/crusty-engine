---
name: run-app
description: Launch this engine's editor, standalone client, locomotion demo or multiplayer loop, smoke-test it for panics, and capture screenshots for visual checks. Use when a change must be seen running, not just compiled.
---

## Launch

```bash
cargo run -p game_client --features editor                           # editor
cargo run -p game_client                                             # standalone (default scene, HUD)
cargo run -p game_client -- --scene scenes/locomotion_demo.scene     # standalone, offline demo (WASD, Shift, Space, mouse orbit)
cargo run -p game_client -- --scene scenes/locomotion_demo.scene --stress-anim 300   # 300 animated characters
cargo run -p game_client --release -- --benchmark --uncapped --warmup=100 --samples=500   # benchmark runner → benchmarks/
```

Editor demo path: open `scenes/locomotion_demo.scene` → F5 plays, Escape/F1
releases the mouse. Flags: `docs/FLAGS.md`.

Multiplayer loop (needs the `spacetime` CLI): `scripts/export_windows.ps1`
then `scripts/host_local.ps1` (publishes the module to a local SpacetimeDB
and launches the exported client); `scripts/smoke_packaged.ps1` is the
deterministic packaged smoke test. Runbook: `docs/roadmap/M9.5-COOP-RUNBOOK.md`.

## Smoke run (no human at the keyboard)

```bash
cargo build -p game_client --features editor            # build first; never wrap the build in a timeout
timeout 30 ./target/debug/game.exe > .scratch/smoke.log 2>&1
grep -iE "panic|panicked|validation|error" .scratch/smoke.log
```

Exit 143 is the timeout killing a healthy app — a pass when the grep is
clean. No logger is installed: only `println!` output and panics reach the
log. The run rewrites `editor_layout_crusty.ron` and `window_config.ron`;
do not commit that churn.

## Screenshots (Windows)

```bash
pwsh scripts/screenshot.ps1 -TitleMatch "Rust Game Engine" -Out .scratch/shot.png
```

Matches a top-level window by title substring (floating dock windows have
their own titles). Read the PNG back to look at it; for a comparison against
a reference or a UI-quality verdict, hand both images to Codex per *Visual
Changes and Computer Use* in `CLAUDE.md`.

## Report

State what was verified live (which host, which scene, what you did) apart
from what is test-proven; the roadmap close-out keeps those columns separate.
