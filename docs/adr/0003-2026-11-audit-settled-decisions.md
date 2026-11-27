# ADR 0003 — Settled decisions of the 2026-11 audit

**Status:** accepted (2026-11, owner plus a three-model council)

## Context

The 2026-11 audit set the engine's goal (`docs/roadmap/ROADMAP.md` ▸ Goal:
one engine for a large seamless-world MMO and for large offline open-world
games) and asked whether any of the engine's libraries or structural choices
stood in the way. The roadmap M11–M20 was written on the answers below.

## Decision

1. **Keep Vulkano 0.35.** Its source has multi-draw indirect with count,
   descriptor indexing, compute, mesh shaders, acceleration structures, ray
   query, ray tracing pipelines and raw sparse binding. Gaps are reachable
   through raw `ash` calls.
2. **Keep `hecs`, behind the engine's ECS API.** It does not block parallel
   systems, replication or streaming; the engine's `&mut World` signature
   does (M12.6).
3. **Keep SpacetimeDB,** subject to the M13 gate. The 150-mover ceiling is
   the module's own code. Scale beyond one database comes from one database
   per zone.
4. **Rendering order:** GPU-driven renderer, clustered lights and cascades;
   then terrain, level of detail and camera-relative rendering; then
   temporal anti-aliasing and upscaling; then probe-based global
   illumination; virtual geometry and texturing last.
5. **Hardware ray tracing is optional,** a player setting, never required.
6. **Terrain is authored on a heightfield** and baked to cell meshes.
7. **The editor follows Unreal** for most surfaces, Godot for scene
   composition and in-editor documentation, and Unity for the game UI
   designer and the command palette (`docs/EDITOR-UX.md`).
8. **Offline games save to local files;** they do not bundle a database.
9. **Data tables are typed RON,** published into SpacetimeDB as rows at
   deploy.
10. **Portable leaf crates:** anything that may run on the server
    (`game_shared`, the graph crates, navigation) stays free of the engine
    and is checked for wasm32 in CI.
11. **Rust owns authority.** Graphs and scripts issue commands; they never
    bypass the command and event interface.
12. **Inspector first, graphs when needed.** Simple cases never require a
    graph editor.

## Consequences

- No library replacement appears in the roadmap; M12 upgrades in place.
- The order in decision 4 fixes the sequence of M14 → M15 → M16 → M20 and
  parks virtual geometry and texturing as unscheduled.
- Reopen a decision only with new evidence, in a new ADR that supersedes
  this one for that point.

## Alternatives considered

Replacing Vulkano with `ash`, `hecs` with `bevy_ecs`, or SpacetimeDB with a
custom renet/QUIC server (the fallback in ADR-014). Rejected: the audit found
each limit to be in the engine's own code, not in the library.
