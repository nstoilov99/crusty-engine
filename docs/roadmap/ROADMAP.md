# Roadmap

Where the engine is, what is open, and the remaining tasks in phase order.
Finished tasks — plans, close-outs, binding rulings and **deferred ledgers** —
live in [COMPLETED.md](COMPLETED.md); per-task plan documents are the
`VULKANO-<N>-<NAME>.md` files beside this one. Phase ordering is flexible: a
later task is pulled forward when it unblocks real pain.

---

## Where we are (2026-11)

Fifty-odd tasks in, the engine is a working editor + runtime + multiplayer
stack. What exists, by subsystem (details in `docs/ARCHITECTURE.md`):

- **Rendering** — Vulkano 0.35 deferred PBR pipeline (G-buffer, directional
  light with PCF shadows, SSAO, bloom, tonemapping), render graph with
  graph-owned pass execution and debug read/write cross-checks, separate
  render thread fed by an owned `FramePacket`, shader hot-reload with material
  instances, GPU compute particles (inspector-authored), debug draw, frustum
  culling, profiling + benchmark runner. Z-up game space, converted to Y-up
  only in `render_adapter`.
- **ECS & scheduling** — custom wrapper over hecs, per-system access
  declarations with a validated sequential scheduler and run criteria,
  `EntityGuid` identity, plugin system (`EnginePlugin`, staging
  `PluginContext`, restart-only activation from `project.ron`).
- **Scenes & assets** — RON scenes and basic prefabs, asset manager with hot
  reload, glTF / FBX / OBJ import, rendered thumbnails, `.pak` packaging and
  Windows export with standalone / mp-client / mp-server targets, cooked
  collision chunks, deterministic greybox world generator.
- **Physics** — Rapier 0.25, cooked collision chunks streamed by cell, capsule
  character controller with presentation interpolation and step handling.
- **Animation** — skeletal animation with GPU skinning (palette ring,
  instanced draws, update-rate throttling), `.animgraph` assets: state
  machines, blend trees, 1D/2D blend spaces, rule graphs on transitions,
  state aliases, nested machines, and the pipeline root (State Machine →
  Layered Blend Per Bone / Play Once → IK Chain → Output Pose); two-bone and
  look-at IK with foot placement v2; Mixamo import with by-name remap;
  locomotion demo scene.
- **Node graphs & scripting** — three engine-independent, wasm32-checked
  crates (`node_graph_types`, `node_graph_exec`, `curve_asset`); script
  graphs (`.graph`) run through `GraphRunner` and emit `Effect`s, never
  mutating the world directly; a 46-node standard library, subgraphs, curves
  and a Timeline latent; graph editor on crusty-gui's `Canvas` with palette,
  find, F8 error navigation, auto-layout, orthogonal wire routing and
  debugging (breakpoints, watches, flow bubbles, pause/step); a node-graph
  design system with a CI lint.
- **Editor** — in-house `crusty-gui` (egui removed 2026-07): dock with
  per-document layout profiles, hierarchy / inspector / asset browser /
  viewport with gizmos / console / profiler / variables / anim preview, play
  mode with snapshot-restore, command-history undo for hierarchy and gizmo
  operations, entity clipboard with GUID remap, semantic theme tokens with
  four presets, settings windows, build dialog, Play As Client / Listen
  Server launcher.
- **Multiplayer** — SpacetimeDB module (`server/game_module`, WASM),
  server-authoritative movement shared through `game_shared` with client
  prediction and a WASM parity suite, combat slice (abilities, casts,
  death/respawn, projectiles, crusty HUD), interest management (3×3 near /
  7×7 far tiers), `tools/net_bots` load harness, `net_config.ron`, local
  host loop and Maincloud publish verified. Known ceiling: ~150 active movers
  per module before sim CPU saturates (`M8-LOAD-REPORT.md`).
- **Audio & input** — Kira playback (basic), input action maps with gamepad.
- **Tooling** — CI on Windows + Linux (build, test, clippy `-D warnings`,
  fmt), design lint, wasm32 checks for `game_shared` and the graph crates;
  `scripts/` for export, local hosting, smoke and soak.

## Status board

| Task | Title | Status |
|---|---|---|
| 1–8 | 2D foundation (window, pipeline, sprites, camera, batching) | ✅ |
| 9–17 | 3D rendering, PBR, shadows, assets, ECS, scenes, deferred pipeline | ✅ |
| 18, 18.5 | egui integration | ✅ — replaced by in-house crusty-gui (2026-07) |
| 19–25 | Physics, hierarchy, inspector, gizmos, asset browser, play mode, Windows export | ✅ |
| 26–29.5c | Profiling, frustum culling, CI, debug draw, FBX/OBJ import, thumbnails | ✅ |
| 30–31 | Skeletal animation, audio (Kira) | ✅ |
| 32–36 | Access declarations, input actions, crate split, render graph, render thread | ✅ |
| 37–39 | Deferred PBR + post-processing, particles, shader hot-reload + material instances | ✅ |
| 39.4 | Editor UI theme (egui) | ⛔ superseded → M10 |
| 39.5 | Asset / systems editor windows | ⚠️ partial — remaining window types land opportunistically |
| 39.8 | Plugin system & module registry | ✅ |
| RC #1–#6 | Refactor checkpoints | ✅ |
| M0–M10 | Multiplayer foundation (SpacetimeDB), packaging, editor net play, design system v1 | ✅ (M1 folded into M10) |
| 🎯 | Networked Co-op Slice milestone | ✅ achieved (hour soak waived) |
| 40 | Node graph framework & custom node SDK | ✅ |
| 41 | Animation graph | ✅ PR #8 |
| 41.5 | Animation at scale + IK | ✅ PR #9 |
| 41.6 | Locomotion demo | ✅ PR #10 |
| 41.7 | Animation pipeline root | ✅ PR #11 — user live check pending |
| 42 | Save/load & runtime persistence | 🔀 networked part → M5; local save deferred |
| 43 | Scene management & transitions | 🔀 zone lifecycle → M4; transitions deferred |
| 44 | Asset cooking & level streaming | 📋 |
| 45-A | Graph execution core (visual scripting v1) | ✅ |
| **45** | **Visual scripting (tail on 45-A)** | 📋 **next** |
| **45.5** | **Node graph editor v2 — refactor & polish** | 📋 **next** |
| 46–49 | Terrain, sky/atmosphere, cascaded shadows, IBL/GI | 📋 |
| 50–54 | Material / VFX / audio graphs, prefab variants, workflow polish | 📋 |
| 55 | In-game UI framework | 🔀 thin HUD → M7; framework deferred |
| 56–57 | Navigation & AI, behavior tree editor | 📋 |
| 58 | Parallel ECS execution | 📋 deferred — no longer gates networking |
| 58.5 | Multi-window viewport & editor tabs v2 | 📋 |
| 59 | Networking foundation | ⛔ superseded by M0–M8 |
| 60 | Production hardening & ship a game | 📋 |

## Current focus

- **Next:** Task 45 (visual scripting — gameplay node API, debugging, polish
  on the 45-A runtime), then Task 45.5 (node graph editor v2; re-audit
  `docs/mockup/AUDIT.md` at kickoff).
- **Awaiting the user:** Task 41.7 live check — demo parity at `aim = 0`,
  upper-body hold at `aim = 1`, `--stress-anim 300` within 5 % of 41.5.
- **Proposed order after that** (confirm before acting): 54-A component
  schema + Inspector undo → 44-A asset cooking → 46 / 47 / 48 world building
  → 50 material editor → 51 VFX graph → 52 prefab variants → 53 audio graph
  → 56 / 57 navigation + behavior trees → 55 game UI → 49 lighting → 58 /
  58.5 → 60. Rationale per task under **Now (2026-11)** below.

## Open follow-ups from completed tasks

Items the finished tasks deliberately deferred and nobody owns yet. Each
owning task's section below lists them; the full ledgers are in
COMPLETED.md. Highest-leverage first:

1. **Inspector edits bypass undo.** `InspectorPanelCtx` writes components
   directly; `CommandHistory` is not involved (verified 2026-11 in
   `editor/inspector_crusty.rs`). Task 54's text assumed otherwise. → 54-A.
2. **Plugin-staged resources reach only the startup world.** Scene tabs
   (`fresh_scene_world`) lack `GraphPlanCache` / `CurveCache` /
   `GraphLogSink`, so graph scripting in a tab-opened scene likely never
   runs (41.7 ledger). → Task 45, first package.
3. **`event_input_action` has one exec output** (Pressed only); Released /
   Held and the analog payload need a `type_version` bump with migration
   (45-A ledger). → Task 45.
4. **`GraphScriptRunnerSystem` understates its writes** (spawn/despawn,
   physics). Safe sequentially; a structural-access bit on
   `SystemDescriptor` is a prerequisite for Task 58.
5. **Vulkan validation layers are not enabled** in `VulkanContext::new`
   (RC #6 verified resize "with zero validation errors" by enabling them
   ad hoc). → add an opt-in flag with the next rendering task (47 / 48).
6. **Animation:** jump / fall / land clips still placeholders; strafe axis
   unused; FK/IK split so ground rays sample this frame's foot; pelvis-drop
   cap; Play Once cast clip for the demo; additive layers and mesh-space
   blending (41.6 / 41.7 ledgers). → next animation-content task, or Task 60.
7. **Server sim ceiling ~150 active movers per module** (M8 load report).
   → sharding / interest tuning when a real world needs it (Task 60).
8. **Plan-cache invalidation is wholesale**; **array constants are not
   authorable on canvas**; warnings and errors share one badge (45-A
   ledger). → Tasks 45 / 45.5.
9. **`physics_rapier` cannot be stripped from exports** without a component
   registry (39.8 ledger). → falls out of 54-A's schema.

---

## Phase 11: Node Graph Foundation & Game Architecture (Tasks 40–45)

Tasks 40, 41, 41.5, 41.6, 41.7 and 45-A are complete — see
[COMPLETED.md](COMPLETED.md). The remaining tasks of the phase follow.

### Task 42: Save/Load & Runtime Persistence
**Status:** 🔀 Absorbed into Task M5 for networked state (SpacetimeDB tables
are the save system). A local single-player save format remains deferred.
**Prerequisites:** Task 41

**Deferred scope:** separation of the editor scene format (full-fidelity RON
with `EntityGuid`, editor metadata) from a game save format (runtime state
only — player position, inventory, quest flags, world mutations) defined on
`game_shared` types; checkpoint / quicksave slots with metadata; save/load UI
hooks for Task 55. Delta saves against the base scene; versioning.

**Now (2026-11):** only relevant for an offline / single-player product. If
Task 60 ships the co-op slice, SpacetimeDB persistence covers it and this
task stays closed. The offline locomotion demo (`--scene`) is the only
consumer today and needs nothing.

---

### Task 43: Scene Management & Transitions
**Status:** 🔀 Absorbed into Task M4 (zone/chunk lifecycle). General
transitions / loading screens stay deferred.
**Prerequisites:** Task 42

**Deferred scope:** additive scene loading (UI scene over game scene),
transition effects (fade, crossfade), per-scene settings (ambient, fog,
post-processing overrides, sky), scene registry + asset-browser integration,
main menu → loading screen → level flow, scene unloading with asset cleanup.

**Now (2026-11):** `WorldStreamer` (M4) already loads and unloads cells with
a budget and GUID-stable reload; `SceneRegistry` exists in the editor. The
remaining piece is presentation — a loading screen on the crusty HUD and a
fade pass — and belongs with Task 55 (game UI) or Task 60.

---

### Task 44: Asset Cooking & Level Streaming
**Status:** 📋 Planned
**Duration:** ~1.5–2 weeks
**Prerequisites:** none hard; benefits from Task 46 as a consumer

**⚠️ High-risk scope** — split into **44-A Asset Cooking Pipeline** and
**44-B Streaming & pak v2** at kickoff.

**What you'll build:**
- Cooked asset format: convert raw editor assets (glTF, PNG, RON) into optimized runtime format (pre-processed meshes, compressed textures, binary scene data)
- Import rules: per-asset-type processing settings (texture compression format, mesh LOD generation, audio quality)
- Dependency tracking: know which assets depend on which (material references texture, scene references mesh)
- Cache invalidation: re-cook only assets whose source files or dependencies changed
- Asset versioning: cooked assets include version number, outdated assets trigger re-cook
- Trigger-volume-based level streaming: `StreamingVolume` component, async load/unload scene chunks when player enters/exits
- Background loading with progress reporting (loading screen integration)
- Upgrade `.pak` format (Task 25) to support cooked assets
- Replace load-whole-pak-into-RAM with mmap/ranged reads (pak size must stop costing RAM before content grows)
- Multiple paks mounted in priority order: base + patch paks (ship updates as small overlay paks) — same mechanism serves DLC
- Chunk paks by locality (per-zone / per-asset-type) so streaming reads are contiguous and platform install-chunks map cleanly

**Now (2026-11):** two precedents already exist and should be generalized
rather than re-invented — `tools/collision_cooker` (M2) cooks chunks with a
version stamp, and `WorldStreamer` (M4) streams cells under a budget. The
export pipeline (`tools/pak_tool`, `scripts/export_windows.ps1`, M9 targets)
is the consumer. Shipped builds also need **precompiled SPIR-V** once Task 50
generates shaders (shaderc is editor-only), so cooking is a prerequisite for
50 in packaged builds.

- **44-A (cooking):** mesh/texture cook with BC compression, dependency
  graph, incremental re-cook keyed by content hash, cooked-asset version.
  Acceptance: cold start of the exported locomotion demo at least halves;
  touching one texture re-cooks only its dependents.
- **44-B (streaming + pak v2):** mmap paks, priority-ordered mounts, locality
  chunking, `StreamingVolume` on top of `WorldStreamer`. Acceptance: the
  greybox world streams from a pak without the whole file in RAM; a patch pak
  overrides one asset.

---

### Task 45: Visual Scripting (Node-Based)
**Status:** 📋 **Next.** Tail on the Task 45-A runtime (`VULKANO-45A-GRAPH-EXECUTION-CORE.md`): the interpreter, `GraphRunner` component, events, control flow, latents and the 46-node starter library exist. This task widens the gameplay surface and closes the 45-A ledger. Bullets under *Original plan* predate 45-A where they conflict (there is no `ScriptComponent`; `#[derive(ScriptNode)]` exists but production nodes use explicit descriptor lists).
**Duration:** ~2–2.5 weeks
**Prerequisites:** Task 45-A

**Now (2026-11) — scope, in package order:**
1. **Plugin resources on every world.** Install plugin-staged resources
   (`GraphPlanCache`, `CurveCache`, `GraphLogSink`) into `fresh_scene_world`
   so graphs run in tab-opened scenes (41.7 ledger). Test: a scene opened in
   a second tab ticks `runner_demo.graph`.
2. **Input event completeness.** `event_input_action` gains Released / Held
   exec outputs and an analog payload pin — `type_version` bump + migration
   (`NodeRegistry::register_migration`), saved graphs upgraded on load.
3. **Gameplay node API** (the main body): generic component get/set for the
   `ComponentData` set, physics queries (raycast, overlap), spawn prefab /
   despawn, play sound, **animation parameter bridge** (write `.animgraph`
   parameters — the only sanctioned gameplay → animation path), `event_custom`
   with an explicit target pin, timers, string/format nodes. Every node:
   descriptor doc, `PureNode`/`ImpureNode` impl, `register_impls`, test in
   `crates/node_graph_exec/tests/std_library.rs`.
4. **Authoring gaps:** array constants editable on canvas; expression node
   (inline `health * 0.5 + armor`) compiled to pure nodes; warnings vs errors
   badges.
5. **Runtime hygiene:** plan-cache dependency index (invalidate only plans
   that inlined the edited graph/curve); declare the runner's structural
   access honestly (prerequisite for Task 58).
6. **Realm discipline check (D8):** a CI test that every `realm = Server`
   node has no engine dependency — keeps the SpacetimeDB-side execution
   option open without building it.

**Acceptance:** the locomotion demo's gameplay hooks (jump SFX, death →
respawn, `aim` toggle) are authored as a `.graph` with zero new Rust;
`runner_demo.graph` exercises every new node; wasm32 CI stays green;
`docs/KNOWLEDGE.md` ▸ Graph Execution Gotchas updated.

**Original plan (reference):**

Second consumer of the Node Graph Framework.

*Node Classification:*
- **Pure nodes** (Add, GetPosition, Compare, Distance): compute values with no side effects. No exec pins required. Can be evaluated on hover in the editor for debugging. Cacheable. `realm = "shared"` by default.
- **Impure nodes** (SpawnEntity, PlaySound, ApplyDamage, SetTransform): emit commands/events through the command buffer. Require exec flow pins. Execution order matters. Realm must be explicitly declared.

*Built-in Node Library:*
- Event nodes: on_start, on_update, on_collision, on_trigger, on_input_action
- Flow control: branch, for-each, sequence, gate, delay, do-once
- Data nodes: get/set component, get resource, math, comparison, variable get/set
- Action nodes: spawn entity, despawn entity, play sound, apply force, set transform, send event
- Entity reference nodes: self, find by name, find by tag
- Debug nodes: print, draw debug line/sphere

*Debugging:* execution-path highlighting, breakpoints, watch window — shipped
by the GS design pass and 45-A; remaining polish is Task 45.5.

---

### Task 45.5: Node Graph Editor v2 — Refactor & Polish
**Status:** 📋 Planned (user-requested 2026-08-02: "refactor it again and improve")
**Duration:** ~1.5–2 weeks
**Prerequisites:** Task 45-A (graph execution core) and ideally Task 45 — most
of the deferred ledger unblocks only once the evaluator and the `NodeInst`
schema additions exist.
**Note (2026-08-14):** the GS design pass (commits `19bf0c6`..`ef0e8a3`; its
spec was never committed — `docs/mockup/AUDIT-2026-08.md` is the surviving
record) already shipped several items this ledger listed — flow bubbles,
watches, persisted breakpoints + full pause/step debugging, the config-row
band, variables groups/filter/arrays, and the curve-editor v2 (tangent
modes). Re-audit this ledger at kickoff against `docs/mockup/AUDIT-2026-08.md`,
whose 34-item judgement list (icon accent tint per-preset, motion tokens
unwired, segmented-control focus/pressed states, heading type slot, L2/L3 band
spec amendment, etc.) is now part of this task's backlog.

One consolidated pass over everything the Task 40 + design-system +
input-model arcs deliberately deferred. The authoritative backlog is the
deferred ledger in `docs/mockup/AUDIT.md` (close-out + Pass C sections);
snapshot of it as of scheduling:

- **Unblocked by 45-A** (do first): node rename via `NodeInst.title`
  override + F2 · breakpoint persistence in the sidecar + debug visuals
  (watch chips, execution trace, PAUSED, tinted taken path) · flow bubbles ·
  collapse-to-subgraph inner wiring (`graph_input`/`graph_output`) ·
  per-instance subgraph inspection.
- **Input-model residue**: Q straighten (needs geometry access from the
  state layer — small architecture decision) · Shift+Del exec-chain heal ·
  quick-place (`descriptor.quick_key`, three crates) · Unreal mouse profile
  consumption + Ctrl+drag-from-pin move-all-connections (multi-edge
  ConnectDrag) · Alt+click group-title boundary break (title-region split).
- **Design-system residue**: crossings rendering (Gap/Arc/Circle over the
  existing broadphase cap) · bundling proper (perpendicular offsets, merge,
  max-8) · Vec2/3/4 two-line axis fields · L1 value edit popup ·
  on-canvas color picker · `preserved` distinct visual · full
  asset-reference field on canvas · pinned nodes (auto-layout exemption).
- **Structural refactors earned by three arcs of accretion**:
  `graph_editor_crusty.rs` has grown past 4k lines — split into
  draw/interact/menu modules · edges sort-canonicalization (the recorded
  next diff-noise source) · crusty Phase 17 (retained tessellation /
  culling) when the 2k-node budget is actually approached · revisit the
  glass-vs-alpha unsure list and the palette 0.96 ruling.

**Now (2026-11):** the anim-graph arcs (41 → 41.7) added `CanvasScope`,
breadcrumbs, per-scope palettes, anchored refusals + warnings and the Details
/ Variables / Anim Preview dock panels on the same editor — re-audit with
those in place; several 2026-08 items may be closed or moved. Per-scope
bookmarks and the persisted machine-scope view (41.7 ledger) join this
backlog. Keep the file split first: every later item lands in a smaller file.

---

### Task 54-A: Component Schema & Inspector Undo *(proposed split from Task 54)*
**Status:** 📋 Proposed (2026-11) — pulled forward because four ledgers
converge on it
**Duration:** ~1–1.5 weeks
**Prerequisites:** none

**Why now:** the Inspector hand-codes one section per component, tracks
presence in a 16-bit field, knows `game_shared` types by name, and writes
components directly with no `CommandHistory` entry (verified 2026-11). The
plugin system cannot strip `physics_rapier` from exports (39.8 ledger),
prefab property overrides (52) need per-field diffs, and generic undo (54)
needs a field model. One component registry solves all four.

**What you'll build:**
- A component schema: name, fields (type, range/units, group, flags), registered by the engine, `game_shared`, and plugins — the `ComponentData` enum in `scene/scene_format.rs` becomes one consumer of it.
- Inspector drawn from the schema (hand-coded sections remain only where a custom widget earns it, e.g. colliders).
- `CommandHistory` integration: every Inspector edit, add and remove is a command with a verb/object label; `InspectorPanelCtx` gains the history.
- Presence bitfield removed; "Add Component" lists registry entries.
- Tests: schema round-trip for every registered component; undo/redo of an Inspector edit restores bytes.

**Acceptance:** editing any component field in the Inspector appears in Edit
▸ Undo with a label; adding a component in `game_shared` requires no engine
editor change; `cargo test -p rust_engine --features editor inspector` green.

---

## Phase 12: World Building (Tasks 46–49)

### Task 46: Terrain System
**Status:** 📋 Planned
**Duration:** ~2 weeks
**Prerequisites:** Task 44-B for streaming; otherwise none

**What you'll build:**
- Heightmap-based terrain (16-bit PNG import for fine elevation control)
- Chunked mesh with distance-based LOD (near chunks high detail, far chunks simplified)
- 4-layer texture splatting with blend map (paint different textures: grass, dirt, rock, sand)
- Terrain brush tools in editor: raise, lower, smooth, flatten, paint texture, set height
- Rapier heightfield collider for physics (characters and objects interact with terrain)
- Terrain normals computed from heightmap for correct lighting
- Terrain material with PBR support (normal mapping, roughness variation per texture layer)
- Terrain chunk streaming integration with Task 44 (load/unload terrain chunks by distance)

**Key concepts:** heightmap (2D grid of elevation, 16-bit), chunk (e.g. 64×64
vertices, independently renderable and LOD-switchable), splat map (RGBA
blend weights), brush (raycast to surface, modify heightmap/splat in a
radius).

**Now (2026-11):** `tools/greybox_gen` already generates terrain-like meshes
and the collision cooker cooks them per cell — the terrain asset should slot
into the same cell grid (`world_grid.rs`) so `WorldStreamer` streams terrain
chunks like any other chunk. Height is **Z** (Z-up); brush raycasts go
through the existing viewport picking. Brush UI uses theme tokens and must
pass the design lint. Multiplayer: the server authoritative movement runs on
cooked collision, so terrain edits must re-cook their cells (44-A
incremental cook).

---

### Task 47: Environment, Sky & Atmosphere
**Status:** 📋 Planned
**Duration:** ~1.5 weeks
**Prerequisites:** Task 46

**What you'll build:**
- Procedural sky rendering (Preetham or Hosek-Wilkie atmospheric scattering model) or HDR cubemap skybox fallback
- Time-of-day system: sun entity angle drives directional light direction, color, and intensity automatically
- Day/night cycle with configurable speed (or manual time control in editor)
- Distance fog (exponential, exponential squared) and height fog
- Baked environment probes: cubemap capture at specified positions for metallic surface reflections
- Skybox rendering integrated into render graph (drawn after opaque geometry, before transparent)
- Editor controls: time-of-day slider, fog parameters, sky model selection

**Now (2026-11):** a sky pass is one more `DeferredPass` — RC #6 made that
three sites (pass file, constructor + `passes_mut()`, one `add_pass_with`
block; see the `add-render-pass` skill). Fog belongs in the lighting pass;
the sun is the existing directional light entity driven by a time-of-day
system. Add the opt-in Vulkan validation-layer flag here (open follow-up 5)
and resize-test both hosts. Environment probes feed Task 49's IBL.

---

### Task 48: Cascaded Shadow Maps & Shadow Quality
**Status:** 📋 Planned
**Duration:** ~1.5–2 weeks
**Prerequisites:** Task 47

**What you'll build:**
- Cascaded Shadow Maps (CSM) for directional lights: 3–4 cascades covering near to far range
- Stable cascade splits (logarithmic + uniform hybrid, or manual per-cascade distances)
- Shadow map atlas: all cascades packed into a single large texture
- Per-cascade frustum fitting; cascade blending without visible seams
- Percentage-Closer Soft Shadows (PCSS) or configurable PCF kernel sizes
- Shadow distance fade
- Debug visualization: cascade boundaries drawn using debug draw (color-coded)

**Now (2026-11):** the current single PCF shadow map (Task 37) is the thing
being replaced; keep the lighting pass's shadow sampling behind one function
so the cascade selection is the only change there. Texel snapping matters
more here than usual because the camera orbits (locomotion demo). Animation
throttling (41.5) deliberately ignores the shadow frustum — revisit that
significance input once cascades exist.

---

### Task 49: Advanced Lighting, IBL & GI
**Status:** 📋 Planned
**Duration:** ~2.5 weeks
**Prerequisites:** Task 48

**⚠️ High-risk scope** — may need scoping down to "point shadows + spot lights + IBL + emissive" with full GI deferred to a later task.

**What you'll build:**

*Multi-light support:* point lights with light volumes and cubemap shadows;
spot lights with cone falloff and optional shadow map; tile- or
cluster-based light culling; emissive materials feeding bloom.

*Image-based lighting:* pre-filtered environment map, irradiance map, BRDF
LUT (split-sum), per-scene environment assignment with the Task 47 sky as
the fallback source.

*Global illumination:* screen-space GI or irradiance probes with an editor
bake workflow.

**Now (2026-11):** the deferred pipeline is directional-light only today;
light culling is the architectural piece (everything else is shaders). Do
IBL first — it is the largest visual win for the least risk and reuses Task
47's probes — then point/spot shadows, then GI only if a target scene needs
it.

---

## Phase 13: Visual Authoring Expansion (Tasks 50–54)

### Task 50: Visual Material Editor (Node-Based)
**Status:** 📋 Planned
**Duration:** ~2 weeks
**Prerequisites:** Task 40, Task 39; Task 44-A for packaged builds

Third consumer of the Node Graph Framework.

**What you'll build:**
- Material graph domain: evaluates a graph of material nodes and **compiles to GLSL** that is built into a Vulkan pipeline
- Live preview on a sphere/plane (re-renders whenever the graph changes)
- Material permutation management (transparency, double-sided, masked — each a shader variant)
- Node library: texture sample / coordinate / sampler; math; vector ops; UV (tiling, panner, rotator, triplanar); constants; world data (position, normal, camera, time, screen UV); utility (fresnel, noise, distance); PBR output node
- Inspector-based material editing remains for simple PBR parameter tweaks
- Material graph assets referenced by material instances

**Now (2026-11):** the recipe for a new graph domain is proven twice (script
graphs, anim graphs): a registry, a `GraphDomain`, a compiler with anchored
refusals, a document version. Codegen output goes through the existing
editor shader path (`ShaderCompiler`, shaderc) and the hot-reload registry;
packaged builds need the SPIR-V cooked (44-A). Reuse the anim preview's
render-to-texture for the material ball. Shader variants multiply pipelines
— `PassPipelineBuilder` and the material-instance registry are the seams.

---

### Task 51: VFX Graph (Node-Based)
**Status:** 📋 Planned
**Duration:** ~2 weeks
**Prerequisites:** Task 40, Task 38, Task 50 (shares the codegen path)

Fourth consumer of the Node Graph Framework. Upgrades Task 38's inspector-based particles to node-based authoring (Niagara-style).

**What you'll build:**
- VFX graph domain: each particle's lifecycle as a graph, **compiled to a compute shader** (CPU fallback for small systems)
- Live preview in the viewport while editing
- Node library: emitters (rate, burst, shapes); initialize (position, velocity, size, color, lifetime); update (forces, noise, drag, orbit, attract, size/color over life); render (billboard, mesh, ribbon, lit); conditions (kill rules, scene collision); math / random / curve sample
- Inspector-based authoring remains for simple emitters; subgraphs for reusable behaviours

**Now (2026-11):** the GPU compute particle system (Task 38) is the runtime
target; the graph replaces its fixed update kernel with generated code. Do
Task 50 first so GLSL generation, variant caching and SPIR-V cooking exist.
`.curve` assets (45-A) are the curve-sample node's data.

---

### Task 52: Prefab Variants & Nested Prefabs
**Status:** 📋 Planned
**Duration:** ~1.5 weeks
**Prerequisites:** Task 54-A (field schema for per-property overrides)

**What you'll build:**
- Nested prefabs: prefab instances inside other prefabs
- Per-instance property overrides without breaking the prefab link; Apply / Revert workflows
- Hierarchy indicator for prefab instances; drag from asset browser to instantiate
- Inspector override visualization: modified properties highlighted, per-property revert
- Prefab asset type with dependency tracking

**Now (2026-11):** `scene/prefab.rs` has `load`/`save`/`instantiate(_with_overrides)`
and nothing links an instance back to its source. Overrides are a per-field
diff, which is why the component schema (54-A) comes first. GUID remap on
paste only remaps `Parent.parent_guid` today — nested prefabs need the
general GUID-field remap the schema makes possible.

---

### Task 53: Audio Graph (Node-Based)
**Status:** 📋 Planned
**Duration:** ~1.5–2 weeks
**Prerequisites:** Task 40, Task 31

Fifth consumer of the Node Graph Framework (MetaSounds-style).

**What you'll build:**
- Audio graph domain evaluated on the audio thread at sample rate; asset referenced from `AudioSource`
- Node library: sources (clip, random selector, sequence, loop/one-shot); mixers (volume, pan, crossfade); DSP (reverb, delay, filters, distortion, chorus); spatial (3D position, attenuation curve, occlusion); logic (parameter-driven switching, blend by parameter); envelopes; output bus
- Inspector-based audio stays for simple SFX and music

**Now (2026-11):** Kira owns the audio thread; the graph compiles to a Kira
track/effect chain or to a custom DSP node registered with Kira — decide at
kickoff. Parameters follow the anim-graph pattern (gameplay writes named
parameters; the graph reads them). Footstep anim events (41) are the first
producer to wire.

---

### Task 54: Editor Workflow Polish
**Status:** 📋 Planned (schema + Inspector undo split out as Task 54-A)
**Duration:** ~1.5 weeks
**Prerequisites:** Task 54-A, Task 52

**What you'll build:**
- Full undo/redo coverage for the remaining editor operations: hierarchy reparent/reorder, entity create/delete, component add/remove, prefab overrides, graph operations, gizmo operations — every mutation a labelled command
- Multi-entity selection with box select in the viewport; batch move / delete / duplicate / group / hide
- Global search across hierarchy, assets and components; recent files / scenes
- Remaining preferences and project-settings items not covered by M10's settings windows (key bindings, grid/snap, gizmo sizes, auto-save)

**Now (2026-11):** M10 shipped the settings windows, theme presets and
Preferences ▸ Graph; crusty-gui owns layout persistence (per-document
profiles). What remains is coverage, not infrastructure. Audit the command
set against every editor mutation before writing code; the `Edit` menu's
verb/object labels are the contract.

---

## Phase 14: Production Features (Tasks 55–57)

### Task 55: In-Game UI Framework
**Status:** 🔀 Thin-HUD slice absorbed into Task M7 (crusty-gui action bar / cast bar / target frame). The general framework stays here.
**Duration:** ~2 weeks
**Prerequisites:** Task 54

**What you'll build:**
- Anchored layout system (screen edges / center), containers (stack, grid, absolute)
- Widgets for game UI: text, image, button, slider, progress bar, panel, toggle, text input
- Event system with data binding to ECS components
- Resolution-independent scaling with a reference resolution; DPI awareness
- UI animation: tweens with easing
- UI layout as an asset type

**Now (2026-11):** the "separate from egui" framing is obsolete — crusty-gui
is in-house and already renders the standalone HUD behind the `hud` feature.
Task 55 is therefore a **game UI layer on crusty-gui**: anchored layouts,
data binding, a UI asset and tweens, plus the loading screen / fade that Task
43 deferred. Decide retained vs immediate at kickoff with the HUD as the
measuring stick.

---

### Task 56: Navigation & AI Foundation
**Status:** 📋 Planned
**Duration:** ~1.5–2 weeks
**Prerequisites:** Task 45

**What you'll build:**
- Navmesh generation from scene geometry (walkable surfaces, obstacle carving); editor rebake
- A* on the navmesh; funnel-algorithm path smoothing
- `NavAgent` component; steering behaviours (seek, flee, follow path, wander, avoidance, separation)
- Behavior tree runtime (selector, sequence, parallel, decorator, leaf) with a blackboard; inspector-based authoring
- Debug draw: navmesh overlay, paths, agent radii

**Now (2026-11):** the cooked collision chunks (M2) and the greybox world
are the navmesh input — bake per cell and stream with the chunks. **AI runs
on the server** in multiplayer, so the navmesh query and behavior-tree
runtime must live in `game_shared` and pass the wasm32 check like the
movement controller does. Consider `recast-rs` for generation; keep the
query side dependency-free.

---

### Task 57: AI Behavior Tree Editor (Node-Based)
**Status:** 📋 Planned
**Duration:** ~1.5 weeks
**Prerequisites:** Task 40, Task 56

Sixth consumer of the Node Graph Framework.

**What you'll build:**
- Behavior tree domain on the graph editor (strict hierarchy: parents above children)
- Runtime debug overlay: active branch and per-node status during play
- Node library: composites (selector, sequence, parallel); decorators (repeat, invert, cooldown, blackboard condition); leaves (move to, attack, play animation, wait, patrol, look at, send event); blackboard get/set/compare
- Inspector editing remains for simple AI; game code adds custom leaves

**Now (2026-11):** same domain recipe as Tasks 41 / 50; the anim graph's
state machine is the closest existing editor shape (nodes with ordered
children, badges on wires). Because the runtime is server-side (56), the tree
document must compile in `game_shared` — the portable-crate split used for
script graphs is the model.

---

## Phase 15: Threading, Multi-Window & Validation (Tasks 58–60)

### Task 58: Enable Parallel ECS Execution
**Status:** 📋 Deferred — not needed by the multiplayer stack (the server holds the load; the client stays sequential until profiling says otherwise).
**Duration:** ~1.5 weeks
**Prerequisites:** Task 32; the structural-access bit on `SystemDescriptor` (45-A ledger)

**What you'll build:**
- Rayon-backed parallel scheduler replacing the sequential one; non-conflicting systems run concurrently from the existing access declarations
- Parallel query support for data-heavy systems; worker pool sizing (cores − game thread − render thread)
- Profile parallel vs sequential; identify systems that benefit
- **ECS debugger panel**: system dependency / conflict graph rendered from the access declarations (`Schedule::validate()` already computes it)

**Now (2026-11):** the scheduler swap is mechanical; the risk is systems whose
declared access lies. Before flipping the executor, add the structural-access
bit, audit spawn/despawn sites, and run the schedule validation in CI. The
SpacetimeDB module is single-threaded WASM, so this task only ever helps the
client and the editor.

---

### Task 58.5: Multi-Window Viewport & Editor Tabs v2
**Status:** 📋 Planned (after Task 58 — the extra swapchain + per-window record work wants the threading dust settled first)
**Duration:** ~1–1.5 weeks
**Prerequisites:** Task 58; M10 + the 2026-07-26 "Editor tabs" mockup pass (hide-tabs shipped there)

**What you'll build:**
- **Viewport floats to its own OS window**: render the scene into a second surface/swapchain so a viewport tab can leave the main window like any panel (today the drop is forced back — `app.rs` "Viewports render only in the main window"). This is the render-thread work item; everything below is UI.
- Remaining "Editor tabs" mockup items: overflow "▾ N" chip with MRU popup; ⌘1–9 switching + Ctrl+Tab MRU cycling; F11 focus-viewport; tab right-click menu (Hide Tabs / Close variants / Unpin / Split Right / Move to New Window); per-tab type icons; pinned viewport tab; plain 24px header for single-tab slots

**Why after threading:** a second presentation surface touches the
render-thread contract (`FramePacket`, serial `TargetRenderer` command
buffers); do it once the parallel scheduler has stabilized what crosses
threads. The tab UX items are pure crusty-gui/editor work and can be
cherry-picked earlier.

---

### Task 60: Production Hardening & Ship a Game
**Status:** 📋 Planned
**Duration:** ~2–3 weeks
**Prerequisites:** All previous tasks

The capstone. The engine proves it works by shipping a product, not a
collection of demos.

**What you'll build:**
- Architecture review and API cleanup; deprecated paths removed; naming unified
- Crash handling: panic handler with stack trace, last N log lines, engine state, crash report file
- Graceful degradation: asset load failures show fallbacks, shader errors keep the old pipeline, disconnects don't crash
- CI hardening; documentation refresh (`ARCHITECTURE.md`, `KNOWLEDGE.md`, `DECISIONS.md`)
- **Ship a small complete game** (5–15 minutes) exercising: streamed zones (M4 / 44), physics, anim graphs (41–41.7), audio graph (53), VFX graph (51), visual scripting (45), graph materials (50), game UI (55), persistence (M5 / 42), navigation + behavior trees (56 / 57), co-op over SpacetimeDB (M5–M9)
- Package with the build pipeline (M9 targets) and cooked assets (44)

**Now (2026-11):** the proof point is the **Networked Co-op Slice** (M9.5),
already achieved on packaged builds; Task 60 extends that slice into a game
and closes the animation-content ledger (jump / fall / land clips, a cast
clip, strafe) and the server sim ceiling if the world needs more than ~150
movers.

---

## Optional Tasks (Unnumbered, Added When Needed)

### Scripting Layer (Lua via mlua)
Added only if iteration pain justifies it. Thin command layer for triggers,
quest logic, dialogue, console commands, cutscenes, modding. Non-authoritative
by design — scripts issue commands, Rust validates and executes.

### UI Graph (Node-Based)
Seventh potential consumer of the Node Graph Framework. Only if in-game UI
complexity justifies visual authoring over code-based construction.

### Motion Matching (decided 2026-09-06, not scheduled)
Feasible as one more pose-source node on the pipeline canvas (41.7); a data
problem first (licensed clip sets). Prerequisites and estimate in COMPLETED.md
▸ Task 41.5 note.

### Server-Side Graph Execution (D8 option)
`node_graph_exec` compiles for wasm32 by design so graphs could run inside the
SpacetimeDB module. Keep the CI check (Task 45 package 6); build it only when
a server-authoritative gameplay rule is easier to author as a graph than in
`game_shared` Rust.

---

## Architectural Decisions

1. **Threading contract early, parallelism late** — Task 32 established access declarations; Task 58 enables parallel execution when profiling asks for it
2. **Crate boundaries before features** — Task 34 created `engine` / `game_shared` / `game_client`; everything builds against them
3. **Input actions before consumers** — Task 33 before animation graphs, visual scripting, AI
4. **Node graph framework built once, used six times** — Task 40 serves Tasks 41, 45, 50, 51, 53, 57
5. **Custom Node SDK via proc macros** on a manual `NodeRegistry` API, with `inventory` as one auto-registration backend
6. **Stable IDs for node types and pins** — display names change freely; serialization uses stable slugs
7. **Node metadata: pure/impure, realm, deterministic** — validated at compile time, enforced before networking exists
8. **Inspector authoring always coexists with graph authoring** — simple cases stay simple
9. **Graph versioning and subgraphs from day one** — schema migration for asset evolution, reusable graph functions
10. **Scripting is optional, not scheduled** — Lua via mlua only when iteration pain justifies it
11. **SpacetimeDB integration is Rust-native** — Tasks M0–M8 use `game_shared` behind a backend-neutral command interface (ADR-014); graphs issue commands transparently
12. **Spike before networking** — M0's go/no-go gate validated the backend; the networked co-op slice replaced the single-player vertical slice as the proof point (ADR-016)
13. **Portable leaf crates** — anything that may run server-side (`game_shared`, the graph crates) stays engine-free and wasm32-checked in CI
14. **High-risk tasks are split at kickoff** — 44 and 49 are flagged

---

## Design Principles

### 1. Measure Before Optimizing
Profile before every optimization decision; establish baselines; don't guess the bottleneck.

### 2. Establish Contracts Before Systems
Access declarations before systems that need them; crate boundaries before gameplay code; input action maps before consumers; the graph framework before domain graphs.

### 3. Inspector-First, Graphs When Needed
Every system works with inspector editing; graphs add power for complex behaviour; simple cases never require opening a graph editor.

### 4. One Framework, Many Backends
Shared graph editor (canvas, nodes, pins, wires); domain-specific compilers (interpreter plan, pose graph, GLSL, compute, DSP, behavior tree); domain-specific node libraries; shared proc-macro infrastructure.

### 5. Rust Owns Authority
Engine core, networking and shared rules are Rust; graphs and optional scripts issue commands above that and never bypass the command/event interface.

### 6. Ship Something Real
The networked co-op slice proved the stack on packaged builds; the engine is done when it ships a product, not when all tasks are checked off.

---

**Last Updated:** 2026-11-26 — roadmap restarted from the current state: finished tasks moved to COMPLETED.md, "Where we are" + status board + open follow-ups added, remaining tasks annotated with **Now (2026-11)** notes, Task 54-A proposed.
