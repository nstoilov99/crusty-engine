# Roadmap

Where the engine is, where it is going, and the milestones that get it there.
This is the only planning file: there is no document per task. An agent's
implementation plan goes in its PR description. Finished work, with its
decisions and deferred ledgers, is in [COMPLETED.md](COMPLETED.md).

Milestones continue the M0–M10 series. Tasks are numbered inside their
milestone (M11.1). The old task numbers 42–60 are mapped to their new home
under [Old task numbers](#old-task-numbers).

---

## Goal

One engine, one world technology, two kinds of game:

- **An MMO with a large seamless world**, in the style of World of Warcraft:
  200 or more players visible in one area, thousands per world through one
  database per zone.
- **Large offline open-world games**, in the style of The Witcher 3 or
  Cyberpunk 2077, on the same world and rendering technology.

The design limit for one world is 16×16 km. The weakest supported hardware
(the **floor**) is a GTX 1060 6 GB at 1080p and 30 fps on the low preset with
upscaling.

The libraries stay: Vulkano 0.35, `hecs` behind the engine's ECS API, Rapier,
SpacetimeDB and the in-house `crusty-gui`. The 2026-11 audit found no reason
to replace any of them (see [Settled decisions](#settled-decisions)).

## Where we are (2026-11)

A working editor, runtime and multiplayer stack. Details by subsystem are in
`docs/ARCHITECTURE.md`.

- **Rendering:** deferred PBR on Vulkano (G-buffer, one directional light
  with PCF shadows, SSAO, bloom, tonemapping), a render graph with
  graph-owned passes, a separate render thread fed by an owned `FramePacket`,
  shader hot reload, GPU compute particles, GPU skinning with instanced
  skinned draws.
- **ECS:** a wrapper over `hecs` with per-system access declarations and a
  validated sequential scheduler; `EntityGuid` identity; a plugin system.
- **World:** RON scenes and basic prefabs, cooked collision chunks, a
  deterministic greybox generator, `WorldStreamer` on a 64 m cell grid.
- **Animation:** `.animgraph` assets with state machines, blend spaces, a
  pipeline root with per-bone layering, IK and foot placement.
- **Graphs:** engine-independent graph crates checked for wasm32, script
  graphs that run through `GraphRunner`, a graph editor with debugging.
- **Editor:** `crusty-gui` with docking and per-document layouts, hierarchy,
  inspector, asset browser, viewport gizmos, play mode, undo for hierarchy
  and gizmo operations, theme tokens with a design lint.
- **Multiplayer:** a SpacetimeDB module with server-authoritative movement
  shared through `game_shared`, client prediction, a combat slice, interest
  management, the `tools/net_bots` load harness, packaged co-op verified.

What the audit found between this state and the goal:

| Area | Limit today |
|---|---|
| Verification | The engine has no frame capture and cannot start without a window, so an agent on a machine with no GPU cannot check rendering or the editor. Only `crusty-gui` has golden-image tests. |
| CI | `main` is red: three clippy sites on Linux and one failing editor test on Windows. No check is required. Linux never builds the editor. |
| Device | No minimum Vulkan version is checked and no device feature is enabled; the only extension is `khr_swapchain`. |
| Renderer | One vertex and index buffer per mesh, one descriptor set of four samplers per material, a CPU draw loop, one light, no velocity target. The G-buffer stores absolute world position in 16-bit floats, which loses 0.25 m of precision 256–512 m from the origin. |
| Server | About 150 active movers per module. `move_tick` scans every player row ever created, every input is its own transaction rewriting the public player row, the module is built for size, and the whole world's collision is compiled into it. |
| World | No level of detail, no terrain, full detail only within two cells (about 160 m). |
| Editor | No component schema: the Inspector is hand-coded and its edits bypass undo. No data assets, data tables or terrain tools. Prefab instances keep no link to their source. |
| Build size | `target/` reaches 60 GB because the dev profile keeps full debug info for every dependency at `opt-level = 3`. The SpacetimeDB SDK is 234 of the standalone client's 399 crates. |

## Status board

An agent claims a milestone by opening a draft PR titled with the milestone
id (`M13: …`) before it starts; open PRs are the lock, so check them first.
The **Claimed** column records that PR's link once it merges. Each PR updates
only its own milestone's row and section.

| ID | Milestone | Track | Depends on | Status | Claimed |
|---|---|---|---|---|---|
| M11 | Green `main` and the verification harness | Infra | none | 🚧 M11.1 done; M11.2 next | [#17](https://github.com/nstoilov99/crusty-engine/pull/17) (M11.1) |
| M12 | Build diet, upgrades and shared-API changes | Infra | M11 | 📋 | |
| M13 | Server scale I, ending in the 250-player go/no-go | Server | M12 | 📋 | |
| M14 | Renderer foundation and GPU-driven rendering | Render/World | M12 | 📋 outline | |
| M15 | Clustered lights, cascaded shadows, sky and fog | Render/World | M14 | 📋 outline | |
| M16 | World: camera-relative rendering, level of detail, terrain, cooking, anti-aliasing and upscaling | Render/World | M15 | 📋 outline | |
| M17 | Server scale II: zone databases, handoff, the MMO slice | Server | M13 | 📋 outline | |
| M18 | Editor data model | Editor/Data | M12 | 📋 outline | |
| M19 | Offline slice: saves, navigation, game UI, gameplay nodes | Editor/Data | M16, M18 | 📋 outline | |
| M20 | Global illumination | Render/World | M16 | 📋 outline | |

Awaiting the owner: the Task 41.7 live check (demo parity at `aim = 0`,
upper-body hold at `aim = 1`, `--stress-anim 300` within 5% of Task 41.5),
and the [owner actions](#owner-actions) below.

---

## How work runs

### Tracks

Four tracks. Infra runs alone; afterwards the other three run in parallel,
one agent each. A track owns its paths: other tracks do not edit them except
through a hot-path PR.

| Track | Milestones | Owns |
|---|---|---|
| **Infra** (runs alone) | M11, M12 | `.github/`, `scripts/`, `tools/xtask`, root `Cargo.toml` and `Cargo.lock`, the verification harness |
| **Server** | M13 → M17 | `server/`, `game_shared`, `game_client_net`, `tools/net_bots` |
| **Render/World** | M14 → M15 → M16 → M20 | `engine/src/engine/{core,rendering,adapters,camera,world}`, shaders, the terrain runtime |
| **Editor/Data** | M18 → M19 | `engine/src/engine/editor`, `crates/crusty-gui`, the graph crates, scenes, prefabs, assets |
| **World tooling** (optional fifth agent) | world-tooling tasks of M16, the follow-up ledger | `tools/greybox_gen`, `tools/collision_cooker`, the terrain bake tool, asset cooking |

The world-tooling role shares no files with the renderer; without a fifth
agent, Render/World takes those paths.

**What may run at the same time:**

1. M11, then M12. Nothing runs beside them, because they change CI, the
   manifests and APIs every crate uses.
2. After M12: M13, M14 and M18 together.
3. Then M17, M15 → M16, and M18 → M19 together.
4. M20 last.

Waits across tracks: M19 needs M16 and M18. Terrain brushes in M18 need the
terrain runtime from M16. With one agent, follow the numbers.

**Shared tools.** Infra owns the `tools/xtask` framework. A subcommand
(`server-gate`, `bench`) lives in its own module and belongs to the track
that adds it. Visual scenarios are one file per track under
`tests/visual/`, so tracks do not edit each other's lists.

**Hot paths.** These change only in a small standalone PR that merges before
the work depending on it, and the agent names it when it claims the
milestone: `Cargo.lock`, the ECS API (`engine/src/engine/ecs`), `FramePacket`,
and the paths no track owns (`game_client/`,
`engine/src/engine/{gui,animation,physics,plugins,scripting}`). Paths a track
owns (`game_shared`, for example) are reached the same way by other tracks,
reviewed by the owning track.

### How much detail each milestone has

M11, M12 and M13 are fully specified below: every task has a goal, scope,
what is out of scope, dependencies, acceptance and how to verify it.

M14 to M20 are outlines: goal, demo, dependencies and the numeric gate. The
scope is left open; the gate is not. The agent that starts one of them first
expands it in this file to the level of M11–M13, as a docs-only PR that gets
a council review (`council` skill) and merges before that milestone's first
code PR. An expansion may tighten a gate. Loosening or replacing a gate needs
the owner's approval, recorded in that PR.

### Verification

Agents build on Ubuntu with no GPU. Everything below must also run on
Windows.

- **The harness (M11)** is how an agent checks its own work: a surfaceless
  offscreen mode for the client and the editor, fixed time step and seed,
  frame capture to PNG, golden-image comparison, input replay, and UI
  automation that finds widgets by stable id. Image goldens are compared on
  Linux with Mesa's CPU Vulkan driver (lavapipe), which is the reference
  platform. The agent machine and CI use the same pinned Mesa version.
- **New automation is one Rust program**, `cargo xtask <command>` in
  `tools/xtask`, so there is a single implementation for both systems. The
  existing paired `.sh` / `.ps1` scripts stay.
- **Every PR states what was verified live, what is proven by tests, and
  what was not run.**

Performance is accepted in three tiers, on fixed scenes and camera paths:

| Tier | Where | What it checks | Gate? |
|---|---|---|---|
| 1 | CPU Vulkan, every PR | Counters: draw and indirect calls, triangles submitted and after culling, culled fraction, lights per cluster, GPU memory (at most 3 GB at the floor preset). CPU frame and record time are reported. | Counters are hard gates; times are not |
| 2 | The RTX 5080 runner, fixed settings and driver | GPU time per pass, CPU frame time and record time against a committed baseline | Fails at +5% p50 or +10% p99 |
| 3 | The RTX 5080 runner | The floor preset at 1080p against one eighth of the floor frame budget (4.2 ms) | Recorded only |

Frame rate on floor hardware is certified only on a floor-class card. A new
pass has no baseline to regress against, so the milestone that adds it states
an absolute tier 2 budget for it.

The **floor preset** is a named settings file created in M11.6 (today's
defaults at 1080p); later milestones extend it. GPU memory is the sum of the
engine's own allocations, so it does not depend on the driver. The tier 1
gates apply from the task that adds each counter.

### Merge policy and PR evidence

`CLAUDE.md` carries the line `Merge policy: owner` or `Merge policy: agents`.

- `owner` is the default, and a missing or unreadable line means `owner`:
  agents open PRs and the owner merges them.
- `agents` lets an agent merge its own PR once the evidence below is
  complete.
- Agents never edit that line. A PR that touches it is merged by the owner
  only.
- "Full send" in a request authorises merging that one PR.
- For the PR skills (`babysit-pr`, `full-send`), `Merge policy: agents`
  counts as the user's explicit request to merge.
- Nobody merges on red, and nobody uses `--admin` to get past a check.

Evidence a PR carries before anyone merges it:

1. Required checks green on the head commit, and the branch up to date with
   `main`.
2. The plan and the acceptance checklist in the description, with proof for
   each item.
3. Harness output, once the relevant harness exists and exercises the
   change: counters JSON, screenshots for visual changes, the bot table for
   server changes, the RTX 5080 result for performance changes.
4. A table separating verified live, proven by tests, and not run.
5. An independent review, with each finding fixed or dismissed in writing.
6. No test deleted or loosened. Every new dependency justified in one line.
7. The roadmap status updated in the same PR.

### Closing a task

The task's own PR moves its section from this file to COMPLETED.md, with
the date, the decisions taken, the acceptance results and a deferred ledger,
so that nothing is left to do after the merge. The `roadmap-task` skill lists
every file a close-out touches.

### Owner actions

Only the owner can do these. Agents work around them until they are done.

| Action | Needed by |
|---|---|
| Make these four checks required on `main`, once #17 has merged: `Windows`, `Linux`, `wasm32 (game_shared, graph interpreter)`, `Design lint` | now |
| Install the Claude GitHub App (https://github.com/apps/claude), or leave the AI-review jobs skipping | any time |
| Register the Windows desktop (RTX 5080) as a self-hosted runner under a dedicated non-administrator account, then set the repository variable `GPU_RUNNER` to `true` | M11.6 |
| Choose production server hosting | end of M17 |

The GPU runner runs on `main` after each merge. On a PR it runs for one
commit only, when whoever holds merge authority adds the `gpu-check` label:
the job checks out the head commit recorded in that label event, and the
label is removed automatically on every new push. It never runs for forks.

---

## M11: Green `main` and the verification harness

**Track:** Infra. **Goal:** an agent on a machine with no GPU can prove that
a rendering or editor change works, on a `main` branch whose checks mean
something.

**Demo:** `cargo xtask verify` on Ubuntu captures the locomotion demo and the
editor on lavapipe, compares them with committed goldens, replays a scripted
editor session that clicks widgets by id, and exits 0. The same command runs
on Windows (proven in M11.5).

**Done:** M11.1 Green `main` and required checks
([#17](https://github.com/nstoilov99/crusty-engine/pull/17)); see
[COMPLETED.md](COMPLETED.md#m111-green-main-and-required-checks).

### M11.2 Surfaceless offscreen mode and frame capture

**Goal:** the client and the editor start, simulate and render with no
window and no swapchain, deterministically, and write frames to disk.

**Scope:**
- A `--headless` flag for `game_client`, in the default and the `editor`
  build. It creates no `winit` window and no swapchain; the final image goes
  to an offscreen colour target (`--size WxH`, default 1280×720). Device
  creation today requires a surface (`core/logical_device.rs`) and
  `khr_swapchain`; headless must need neither.
- `--frames N` runs N frames and exits. `--dt` fixes the time step (default
  1/60) for simulation, animation, particles and physics. `--seed` fixes
  every random source. Audio uses a null backend.
- No wall-clock time in headless mode: world streaming and asset loads
  finish synchronously inside the frame that asks for them. `WorldStreamer`
  has a 1 ms wall-clock budget and an IO worker today, which would make two
  runs differ.
- A `gpu_memory_bytes` counter in `RenderCounters`: the sum of the engine's
  own buffer and image allocations.
- `--capture <dir>` with `--capture-frames a,b,c` writes those frames as PNG
  and one `counters.json` holding the `RenderCounters` of each.
- `--validate` turns on the Vulkan validation layers (open follow-up from
  Refactor Checkpoint #6). A validation error makes the process exit
  non-zero.
- In the editor build the UI is laid out and composited into the same
  target.

**Out of scope:** comparing images (M11.3), input (M11.4), any change to the
windowed path's behaviour.

**Depends on:** M11.1.

**Acceptance:**
- `cargo run -p game_client -- --headless --scene
  scenes/locomotion_demo.scene --frames 120 --capture out --capture-frames
  0,60,119 --validate` exits 0 on Ubuntu with lavapipe, with three PNGs and a
  `counters.json`.
- The same command with `--features editor` captures the editor.
- Two runs on one machine produce byte-identical PNGs and counters, for the
  locomotion demo and for the streamed greybox world.
- The windowed client and editor behave as before.
- It builds on Windows. Running it on Windows is proven in M11.5, which is
  the first task with a Windows device to run on.

**Verify:** a test that runs the command twice and compares the output
bytes; run the windowed editor once by hand (`run-app` skill).

### M11.3 Golden images and counter checks

**Goal:** a capture is compared with a committed reference, and a mismatch
produces something an agent can look at.

**Scope:**
- `tools/xtask` and the `cargo xtask` alias in `.cargo/config.toml` are
  created here.
- Scenario lists under `tests/visual/`: scene, build features, frames, size,
  tolerance. First set: the locomotion demo (three frames), the greybox
  world from a fixed camera, and the editor's default layout with a scene
  given on the command line. Scenarios that need clicks come in M11.4.
- Image comparison reuses the tolerance logic of the `crusty-gui` harness
  (`crates/crusty-gui/tests/common/mod.rs`), moved to where both can use it.
- `cargo xtask verify` runs every scenario. A failure writes the actual,
  expected and difference images to `target/visual/`, and CI uploads them.
- Counters are compared exactly with a committed expectation for each
  scenario.
- `cargo xtask verify --bless` rewrites the goldens. A PR that blesses shows
  the before and after images in its description.
- The Linux CI job runs `cargo xtask verify`.
- One Mesa version for the agent machine and CI, recorded beside the
  goldens. `verify` sets `LP_NATIVE_VECTOR_WIDTH=256`, because lavapipe
  otherwise compiles for the host CPU and two machines produce different
  pixels. It stops with a clear message when the installed Mesa differs from
  the recorded one. A Mesa upgrade re-blesses in its own PR.

**Out of scope:** comparing images on Windows. There the run must exit clean
and match the driver-independent counters (draws, triangles, culled
fraction, engine-side memory); images are uploaded, not compared. Also out
of scope: performance numbers.

**Depends on:** M11.2.

**Acceptance:**
- `cargo xtask verify` passes on the agent machine and in the Linux CI job
  from the same goldens.
- Changing one shader constant makes it fail with a difference image.
- CI uploads the three images of a failing scenario.
- It builds on Windows; the Windows run is proven in M11.5.

**Verify:** the deliberate-break check above, run once and reverted.

### M11.4 Input replay and UI automation

**Goal:** an agent drives the game and the editor from a script, and finds
widgets by name instead of by pixel position.

**Scope:**
- A replay file (`.replay.ron`): steps tied to frame numbers. Keys, mouse
  movement, `Click { widget }`, `Type`, `Drag { from, to }`,
  `ViewportClick { world }` (a world position projected through the camera),
  `Capture { name }`, `AssertWidget { id, … }`, `AssertCounter`.
- Events are injected at the engine's input layer, where `winit` events
  arrive, so gameplay and the editor see them the same way. `--replay <file>`
  works with `--headless`.
- `crusty-gui` gives every interactive widget a stable id path, such as
  `dock/inspector/transform/position.x`, and keeps a queryable tree of the
  last frame: id, kind, rectangle, text, state. `--dump-ui <file>` writes it
  as JSON.
- Ids for every existing editor panel. A test fails when two widgets in one
  frame share an id.
- Replay scenarios join the visual scenarios: an editor session (select an
  entity in the hierarchy, edit a transform field in the Inspector, then
  delete the entity and undo the delete), a gameplay session (walk and jump
  in the locomotion demo), the graph editor opened on
  `content/graphs/runner_demo.graph`, and the animation pipeline canvas.
  The undo step uses a hierarchy delete because Inspector edits are not
  undoable until M18.2.

**Out of scope:** recording a replay from live input, automation of native
file dialogs.

**Depends on:** M11.3.

**Acceptance:**
- The four replay scenarios pass in `cargo xtask verify` on Ubuntu.
- `--dump-ui` lists every interactive widget of the default editor layout
  with a unique id.
- Moving a panel in the dock layout does not break a replay that clicks by
  id.

**Verify:** the scenarios themselves; `add-editor-panel` gains the rule that
new widgets need ids.

### M11.5 Windowed smoke test and the agent entry point

**Goal:** the real window and swapchain path is exercised on both systems,
and agents have one documented way to check their work.

**Scope:**
- `cargo xtask smoke`: the windowed client and editor run 120 frames with a
  real swapchain, resize once, and exit 0 with no validation error. It runs
  under `xvfb-run` on Linux and on the desktop on Windows.
- The Linux CI job runs it.
- Windows runtime proof for everything in M11.2–M11.4 is collected here. For
  the hosted Windows job, try Mesa's lavapipe build for Windows; if that
  does not work within a day, the Windows run of `verify` and `smoke` moves
  to the self-hosted runner (M11.6) and the PR says so.
- Update the `run-app` skill and `CLAUDE.md` ▸ Before Making Changes:
  `cargo xtask verify` is the gate for visual and editor work.
  `scripts/screenshot.ps1` remains for a desktop session.

**Out of scope:** new scenarios.

**Depends on:** M11.4.

**Acceptance:**
- `cargo xtask smoke` passes on Ubuntu under Xvfb.
- `cargo xtask verify` and `cargo xtask smoke` pass on Windows, on the
  hosted job or on the self-hosted runner. If neither exists yet, the PR
  lists the Windows run as not run and M11.6 closes it.
- A fresh agent following `CLAUDE.md` alone can run the harness.

**Verify:** run both commands on both systems; record which Windows path was
taken.

### M11.6 GPU runner workflow and baselines

**Goal:** frame timings are measured on real hardware without the owner
doing anything by hand.

**Scope:**
- `.github/workflows/gpu.yml` for a self-hosted runner labelled `gpu-5080`.
  It runs on `main` after each merge, and on a PR when the `gpu-check` label
  is added. The job checks out the head commit recorded in the label event,
  not the branch tip. A second small workflow removes the label on every new
  push. It never runs for forks.
- The job runs only when the repository variable `GPU_RUNNER` is `true`. A
  job whose runner does not exist is not skipped by GitHub: it queues for a
  day and then fails.
- Quality presets as named settings files. `floor` starts as today's
  defaults at 1080p.
- GPU timestamp queries around each render pass. The engine has none today.
- `cargo xtask bench`: fixed benchmark scenes and camera paths at fixed
  settings; per-pass GPU timings, CPU frame time and record time; p50 and
  p99 written as JSON.
- A committed baseline (`benchmarks/baseline-5080.json`, with the driver
  version). The job fails at +5% p50 or +10% p99 and prints the comparison.
  It also runs the floor preset at 1080p and records the result against
  4.2 ms without failing.
- A runner setup page in `docs/` for the owner: dedicated non-administrator
  account, labels, driver pinning.

**Out of scope:** registering the runner (an owner action), new benchmark
scenes for later milestones.

**Depends on:** M11.3.

**Acceptance:**
- With the runner registered, a merge to `main` produces a timing report.
- A deliberate 10% slowdown on a PR with the label fails the job.
- While `GPU_RUNNER` is unset, no GPU job is queued.
- If M11.5 deferred the Windows run, `verify` and `smoke` pass on this
  runner.

**Verify:** the deliberate-slowdown check, once the owner has registered the
runner. Until then this task is test-proven only, and its PR says so.

---

## M12: Build diet, upgrades and shared-API changes

**Track:** Infra. **Goal:** a smaller, current, cleaner base, with every
repository-wide mechanical change made while nothing else is in flight.

**Demo:** a before-and-after table of crate counts and `target/` size, the
harness still green, and the M8 load scenarios passing on SpacetimeDB 2.16.

### M12.1 Remove unused dependencies and put networking behind a feature

**Goal:** the build carries nothing it does not use.

**Scope:**
- Remove `typetag` (no uses). Remove `assets/async_loader.rs` (no callers)
  and the engine's `tokio` dependency with it.
- A `net` feature on `game_client`, on by default, that gates
  `game_client_net` and through it `spacetimedb-sdk`. An offline build
  (`--no-default-features --features hud,graph-scripting`) must not compile
  the SDK.
- Delete code the compiler confirms is dead: candidates are the forward
  pipelines and their shaders in `rendering/3d/pipeline_3d.rs`,
  `TransientResourcePool`, unused parts of `rendering/2d`, and
  `EntityBuilder` in `ecs/world.rs`. Remove only what has no caller.
- Turn off `resvg`'s `text` and `system-fonts` features if no icon contains
  text.

**Out of scope:** replacing any library in use; `tobj`, `puffin` and `tracy`
stay.

**Depends on:** M11.

**Acceptance:**
- `cargo tree` for the offline build has no `spacetimedb-sdk` and no
  `tokio`.
- The PR states the lock-file package count before and after (708 before).
- `cargo xtask verify` and all CI jobs pass, including an offline-build
  check added to CI.

**Verify:** `cargo tree -p game_client --no-default-features --features
hud,graph-scripting -e normal`, then the harness.

### M12.2 Dev profile that does not fill the disk

**Goal:** a debug build that stays debuggable in the workspace's own code
and costs far less disk.

**Scope:**
- Workspace crates: `debug = "line-tables-only"`. Dependencies:
  `debug = false`. Keep the current optimisation levels unless measurement
  shows a better choice.
- Measure a clean build of both feature sets before and after with `du`, on
  Ubuntu, and put the numbers in the PR.
- A short note in `docs/KNOWLEDGE.md` on what grows `target/` (incremental
  data accumulates per session) and how to prune it.

**Out of scope:** changing the release or shipping profiles.

**Depends on:** M12.1.

**Acceptance:**
- A clean debug build of both feature sets is at least 40% smaller than the
  baseline measured in the same PR.
- A panic in workspace code still prints file and line.

**Verify:** the two `du` measurements; a forced panic.

### M12.3 One math library

**Goal:** `glam` only. `nalgebra-glm` pulls a second `nalgebra` (0.32 beside
the 0.33 Rapier needs), so `nalgebra` and `simba` compile twice.

**Scope:**
- Replace `nalgebra-glm` with `glam` in the roughly 40 files that use it,
  and remove the dependency.
- Keep the Z-up conventions and the conversion functions in
  `render_adapter` exactly as they behave now.

**Out of scope:** changing any math, the `transform-gizmo` fork's own
dependencies.

**Depends on:** M12.2.

**Acceptance:**
- `nalgebra-glm` is gone from `Cargo.lock`, and `nalgebra` appears once.
- Every golden image is unchanged, with no re-bless.
- All tests pass, including the WASM parity suite.

**Verify:** `cargo xtask verify` with no blessing; `cargo tree -d`.

### M12.4 Vulkan device features

**Goal:** the device is created with the features the next milestones need,
and the engine knows what the hardware offers.

**Scope:**
- Require Vulkan 1.2 or later. Vulkano already asks for the highest version
  the driver offers; what is missing is the check.
- Required features: multi-draw indirect, draw-indirect count,
  `drawIndirectFirstInstance`, `shaderDrawParameters`, and descriptor
  indexing (runtime arrays, non-uniform indexing of sampled images,
  partially bound, update after bind, variable count). Starting on a device
  without them fails with a message naming what is missing.
- Before enforcing the list, confirm every feature and the limits M14 relies
  on against the pinned lavapipe and a GTX 1060 capability report, and put
  the comparison in the PR. A feature the floor card lacks goes to the owner
  before it is required.
- Optional features, detected and recorded: ray query, acceleration
  structures, mesh shaders, sparse binding.
- A `DeviceCaps` value available to the renderer and printed at start.

**Out of scope:** using any of the features.

**Depends on:** M12.3.

**Acceptance:**
- The engine starts on lavapipe and on the RTX 5080, printing its
  capabilities.
- A unit test covers the missing-feature error.
- Every golden image is unchanged.

**Verify:** `cargo xtask verify` and `cargo xtask smoke`.

### M12.5 SpacetimeDB 2.6 to 2.16

**Goal:** the server stack on the current release, which has the
asynchronous calls between databases that M17 needs.

**Scope:**
- `spacetimedb` in `server/game_module` and `spacetimedb-sdk` in
  `game_client_net` to 2.16; regenerate the client bindings; update the CLI
  version the scripts expect.
- Bump the protocol version and fix what the upgrade breaks.
- `cargo xtask host`: start a local database, publish the module and print
  the address, on Ubuntu and Windows (what `scripts/host_local.ps1` does on
  Windows today).

**Out of scope:** using inter-database calls, any optimisation (M13).

**Depends on:** M12.4.

**Acceptance:**
- The WASM parity suite passes.
- A minimal two-database test confirms that an asynchronous inter-database
  call works on 2.16. M17 depends on it; if it does not work, tell the owner
  now.
- On the agent machine, `tools/net_bots` stays within these bars: uniform-50
  round trip p50 at most 39 ms and p95 at most 75 ms; churn-100 p50 at most
  43 ms; thrash-50 at most 3 swaps per bot in 120 s; zero unexpected
  disconnects. (The M8 report's numbers came from a 16-core Windows desktop;
  these bars are restated for the agent machine.)
- The packaged smoke test (`scripts/smoke_packaged.ps1`) passes on Windows,
  or is listed as not run.

**Verify:** `cargo xtask host`, then `net_bots` for each scenario; the
numbers go in the PR.

### M12.6 Systems take `&World`

**Goal:** the ECS API stops handing every system the whole world mutably,
which is what blocks running systems in parallel later. This is the early
half of old Task 58, done now because it touches about 70 files.

**Scope, as two PRs that each build and pass:**
1. A command buffer for structural edits (`hecs` has `CommandBuffer` and
   `reserve_entity(&self)`). Systems move their spawns, despawns and
   component adds or removes onto it; commands are applied between systems.
2. `System::run` changes from `&mut hecs::World` to `&hecs::World` plus the
   command buffer. A structural-access flag on `SystemDescriptor`;
   `GraphScriptRunnerSystem` declares its real writes. Schedule validation
   runs in CI.

`&mut Resources` stays as it is in this task.

**Out of scope:** a parallel executor and per-resource access declarations
(both come when a profile asks for parallel systems), change detection,
moving off `hecs` 0.10.

**Depends on:** M12.5.

**Acceptance:**
- No system receives `&mut World`.
- Every golden image is unchanged and all tests pass.
- Main-thread schedule time, as the median of three headless runs with
  `--stress-anim 300`, is within 5% of the previous commit. The tier 2
  runner confirms it once it exists.

**Verify:** `cargo xtask verify`; the three-run timing before and after.

---

## M13: Server scale I

**Track:** Server. **Goal:** find out, with numbers, whether one SpacetimeDB
database can hold 250 players and show 200 of them to each other in one
place.

**Demo:** one command runs the gate and prints the table below with a pass
or fail on each line.

**Why the ceiling is 150 today** (`M8-LOAD-REPORT.md` and the 2026-11
audit): `move_tick` iterates the whole `player` table, offline rows
included; each input is its own reducer and rewrites the public `player`
row to bump `last_input_seq`, so every input fans out to every subscriber;
idle players are stepped every tick; the collision step allocates a
`HashMap` per call and casts shapes against every candidate triangle; the
module is built with `opt-level = "z"`; and on SpacetimeDB 2.6 the next tick
was scheduled 50 ms after the previous one finished, not on a fixed clock.

### M13.1 One command measures the gate

**Goal:** every number in the gate comes from one repeatable command, before
anything is optimised.

**Scope:**
- `cargo xtask server-gate`: starts a fresh database, seeds 2,000 offline
  player rows, runs each scenario with 50 bots per process, and prints the
  gate table.
- `tools/net_bots` reports what the gate needs and does not have yet: the
  simulation rate per minute, the number of other full-detail avatars each
  client receives, and the staleness of visible players.
- `move_tick` duration covers the whole reducer body, including the player
  scan (today's stopwatch starts after it). Host-side timing or the module's
  stopwatch logs are both acceptable sources, as long as the command turns
  them into p95 and the maximum by itself.
- Server and bot CPU are reported separately. A run is void when the bots
  miss more than 1% of their own 20 Hz send deadlines, so that a starved bot
  process is never read as a slow server. The agent machine has 8 cores, and
  M8's hotspot-150 used 12.
- The `server-gate` subcommand is the Server track's module in `tools/xtask`.
- A baseline run on the agent machine, recorded in the PR.

**Out of scope:** any optimisation.

**Depends on:** M12.5.

**Acceptance:** the command runs unattended on Ubuntu and on Windows and
produces the full table, with today's failures visible in it.

**Verify:** run it twice; the two tables agree within run-to-run noise,
which the PR states.

### M13.2 Tick cost

**Goal:** `move_tick` does work only for players who need it, on a fixed
clock.

**Scope:**
- Build the module at `opt-level = 3`.
- Iterate online movers only, through a table or index that holds them.
- Skip players with no input and no motion.
- Ticks on a fixed 20 Hz clock. SpacetimeDB 2.16 may already anchor interval
  schedules: measure the cadence from M13.1 first and change the scheduling
  only if it drifts.
- Remove the per-call allocation in `game_shared/src/collision/store.rs` and
  reduce the candidate set before the shape cast.

**Out of scope:** changing what is replicated (M13.3); the f64 shape cast
stays, because the collision regression tests depend on it.

**Depends on:** M13.1.

**Acceptance:**
- The WASM parity suite and the collision battery pass unchanged.
- With 2,000 offline rows seeded, the uniform-50 tick time is within 10% of
  the time with none.
- The PR shows the gate table before and after.

**Verify:** `cargo xtask server-gate`; `cargo test -p game_shared`.

### M13.3 Replication writes

**Goal:** a player's input costs the server one small private write, and
each subscriber receives only what it can use.

**Scope:**
- Inputs stop rewriting the public `player` row. The input sequence
  acknowledgement moves to a row only its owner receives.
- Split the wide `player` row into a narrow, quantised position row that
  changes every tick and a row for everything that changes rarely.
- Clients may send two or three inputs in one call.
- The far tier updates at a reduced rate (5 to 10 Hz).
- Client prediction and reconciliation follow the new rows; the protocol
  version is bumped.

**Out of scope:** more than one database (M17).

**Depends on:** M13.2.

**Acceptance:**
- Two packaged clients play together as before: movement, combat, respawn.
- Prediction error in the bot report does not rise against M13.2.
- The PR shows the gate table before and after.

**Verify:** `cargo xtask server-gate`; the packaged smoke test; the two
client check is listed as verified live or not run.

### M13.4 The gate

**Goal:** a decision, written down.

**Setup:** the Ubuntu agent machine, server and bots on the same machine,
50 bots per process, production collision, inputs at 20 Hz, a fresh database
seeded with 2,000 offline rows. Three runs of 10 minutes each after a 60 s
warm-up. **Every bar must hold in all three runs:** no best-of and no
averaging across runs.

| Scenario | Simulation rate | `move_tick` p95 | Client round trip p50 / p95 |
|---|---|---|---|
| uniform-250 | at least 19.5 ticks/s in every minute | at most 35 ms (maximum 50 ms) | at most 40 / 80 ms |
| hotspot-250: all in one 3×3 near area, each client receiving at least min(N − 1, 200) other full-detail avatars, dispersing at the end | at least 19.5 ticks/s | at most 40 ms | at most 60 / 150 ms |

- Hotspot only: staleness of visible players p95 at most 250 ms, and back
  within the uniform bars 20 s after dispersing.
- Regression: churn-100 round trip p50 at most 43 ms; thrash-50 at most 3
  swaps per bot in 120 s.
- All scenarios: zero unexpected disconnects (scheduled churn cycles
  excluded) and at least 99% of pings returned.
- Soak: 30 minutes of uniform-250 with resident memory growing by at most
  10%.

**If uniform-250 fails:** no area cap can be set. Profile the run and take
the result to the owner before anything else.

**If only hotspot bars fail:** N is the largest hotspot count that passes
every hotspot bar while uniform-250 still passes. Profile the failing run.

- **N is 150 or more, and WASM stepping is not the dominant cost:** N
  becomes the cap per area. Crowds above it are split by layering, the far
  tier runs at a reduced rate, and M17 moves ahead of M15.
- **N is below 150, or WASM stepping is still dominant after M13.2:** spike
  a native Rust movement worker, with SpacetimeDB kept for persistence and
  subscriptions, and take the result to the owner before adopting it.

**Depends on:** M13.3.

**Acceptance:** the three-run tables, the soak result and the decision are in
the M13 close-out in COMPLETED.md.

**Verify:** `cargo xtask server-gate --runs 3 --soak`.

---

## M14: Renderer foundation and GPU-driven rendering

**Track:** Render/World. **Depends on:** M12.

**Goal:** the renderer draws a scene of any size in a fixed, small number of
draw calls, and nothing in it depends on distance from the world origin.

**What it replaces:** per-mesh buffers become shared geometry pools; the
four-sampler descriptor set per material becomes a bindless texture array
and a material buffer; the CPU draw loop becomes compute culling that feeds
indirect draws with a count; world position in the G-buffer is rebuilt from
depth. The velocity target, per-frame jitter and previous-frame matrices
that anti-aliasing needs land here, because they are part of the same
G-buffer change. Kept as they are: the render thread and `FramePacket`
hand-off, graph-owned passes, `PassPipelineBuilder`, and the instance and
palette buffers indexed by instance.

**Demo:** the harness captures a 10,000-object scene and the existing
goldens with the new path.

**Gate:**
- Tier 1, scene `bench_10k` (10,000 static instances, at least 50 meshes
  and 20 materials): at most 16 indirect draw calls for the opaque G-buffer
  pass; the GPU culling result equals a CPU reference cull for every
  captured frame, except objects whose bounds lie within a stated epsilon of
  a frustum plane; the visible geometry matches the pre-M14 renderer's; GPU
  memory at most 3 GB at the floor preset.
- `bench_100k` (100,000 instances) renders with the same draw-call bound.
- The reference scene moved 4 km from the origin matches its render at the
  origin within the golden tolerance (lighting, shadows and SSAO).
- Existing scenes match their pre-M14 goldens within tolerance; any re-bless
  is explained image by image.
- Tier 2: record time for `bench_10k` at most 25% of the pre-M14 renderer's
  on the same scene; G-buffer GPU time not above the pre-M14 renderer's.

## M15: Clustered lights, cascaded shadows, sky and fog

**Track:** Render/World. **Depends on:** M14. Carries old Tasks 47 and 48
and the light-culling half of 49.

**Goal:** many lights, sun shadows that hold over a long view distance, and
an outdoor scene that looks like one.

**Scope notes:** point and spot lights culled into clusters; four shadow
cascades with casters culled per cascade on the GPU and texel snapping (the
camera orbits); a procedural sky with time of day driving the sun; distance
and height fog in the lighting pass. Animation throttling ignores the shadow
frustum today; revisit it once cascades exist.

**Demo:** a captured day-to-dusk sweep over the greybox world, and a night
scene with 1,024 lights.

**Gate:**
- Tier 1, scene `bench_lights` (1,024 point and spot lights): the clustered
  render matches a brute-force reference that evaluates every light per
  pixel, within the golden tolerance; at most 64 lights in any cluster.
- Shadow passes use at most 4 × the opaque pass's indirect draw calls.
- Shadow stability: each cascade's origin snaps to whole texels, and the
  shadow term sampled at fixed world points does not change when the camera
  moves by less than one shadow texel.
- Tier 2 absolute budgets at 1080p on the RTX 5080: lighting with 1,024
  lights at most 1.5 ms; four cascades on `bench_10k` at most 1.5 ms; sky
  and fog at most 0.5 ms.

## M16: World

**Track:** Render/World, with world tooling for the optional fifth agent.
**Depends on:** M15. Carries old Tasks 44 and 46.

**Goal:** an 8×8 km world that streams, holds its detail budget at any view
distance, and has terrain; then the image is stabilised and upscaled.

**Scope notes:**
- Camera-relative rendering, so precision does not depend on position.
  Simulation stays in `f32`: at 8 km the step is about 1 mm.
- A baked level-of-detail chain per cell mesh and one merged proxy per 4×4
  cells for the far field.
- Terrain authored on a heightfield and baked to streamed cell meshes on the
  existing cell grid, collided as a heightfield on client and server. Mesh
  patches for overhangs and caves come later. Unreal 5.8's Mesh Terrain is
  experimental and is not the model for the first version.
- Cell and asset cooking: compressed textures, cooked meshes, precompiled
  shaders, dependency tracking with incremental re-cook, and paks that are
  read by range instead of loaded whole. `tools/collision_cooker` and
  `WorldStreamer` are the precedents to generalise.
- Instanced foliage and props through the GPU-driven path.
- Closing tasks, after everything above: temporal anti-aliasing, then
  temporal upsampling in the same resolve (50–100% render scale). Vendor
  upscalers are out of scope.

**Demo:** a scripted fly-through of a generated 8×8 km world, captured by
the harness.

**Gate:**
- Fly-through on the fixed path, in the harness: process memory at most
  6 GB; GPU memory at most 3 GB at the floor preset; at most 64 indirect
  draw calls and 3 million triangles submitted per frame at the floor
  preset.
- The same fly-through in real time at 60 m/s on the RTX 5080 runner: no
  frame is missing a cell inside the near radius. (A fixed-step run on
  lavapipe would pass this trivially.)
- Precision: a close-up of fine geometry placed 16 km from the origin on
  both axes, the far corner of the design limit, matches the same shot at
  the origin within the golden tolerance.
- Terrain collision passes the client and server parity suite.
- Touching one texture re-cooks only what depends on it; the world streams
  from a pak with no pak fully in memory.
- Anti-aliasing: with a static camera, the converged image has a structural
  similarity of at least 0.95 with a 16× supersampled reference. One second
  after a moving object has passed, the pixels it covered are within the
  golden tolerance of the static background.
- Upsampling at 67% render scale: structural similarity of at least 0.92
  against native-resolution output on the static goldens.
- Tier 2: the fly-through's frame time baseline is committed, with absolute
  budgets for the new passes stated in the expansion.

## M17: Server scale II

**Track:** Server. **Depends on:** M13. If the M13 gate caps an area below
250, this moves ahead of M15.

**Goal:** thousands of players in one world, across databases, without the
player noticing a border.

**Scope notes:** one database per zone, each loading only its own zone's
collision from a table instead of compiling the whole world into the module;
handoff between zones through SpacetimeDB's asynchronous inter-database
calls (outbox tables); clients subscribed to both databases near a seam.
Publishing gameplay data tables to zones is M18's publisher and is not part
of this milestone. Hosting is chosen by the owner at the end.

**Demo (the MMO slice):** a packaged client walks across a zone border in
the middle of a fight, among bots, and nothing is lost.

**Gate, on the agent machine:**
- Four zone databases and 500 bots, with one zone held at the per-area
  population M13 supports and the rest spread over the others. Each bot
  crosses a seam at least 10 times in 10 minutes.
- Every handoff carries a transfer id and ends as exactly one of committed
  or rolled back. No identity is active in two zones for longer than 1 s,
  none stays in transit longer than 5 s, and at the end every identity is
  active in exactly one zone. (Zones commit independently, so a simultaneous
  one-row audit across them would be unsound.)
- Handoff takes at most 500 ms at p95, and the client's position error at
  handoff is at most 0.5 m.
- A player inside the near radius across a seam is visible, with staleness
  p95 at most 250 ms.
- Every zone holds the M13 uniform bars for its own population.
- After one zone database is restarted, its players return at their last
  saved position.
- An eight-zone, 1,000-bot run is recorded as a diagnostic.

## M18: Editor data model

**Track:** Editor/Data. **Depends on:** M12. Carries old Tasks 52 and 54, and
the runtime fix from old Task 45. Editor work follows
[`docs/EDITOR-UX.md`](../EDITOR-UX.md).

**Goal:** the editor is driven by data it can describe: components by a
schema, prefabs by overrides, game data by typed tables.

**Tasks known now, in order:**

1. **M18.1 Plugin resources on every world.** Plugin-staged resources
   (`GraphPlanCache`, `CurveCache`, `GraphLogSink`) reach only the startup
   world, so graphs never run in a scene opened in a tab. Install them in
   `fresh_scene_world`. Test: a scene opened in a second tab ticks
   `runner_demo.graph`.
2. **M18.2 Component schema and Inspector undo** (old Task 54-A). One
   registry of components and fields (type, range, group, flags), fed by the
   engine, `game_shared` and plugins. The Inspector is drawn from it, and
   every edit, add and remove is a labelled command in `CommandHistory`. The
   presence bit field goes. This also lets exports strip `physics_rapier`.
3. **Prefab variants and nested prefabs** (old Task 52): an instance keeps a
   link to its source, overrides are per-field differences against the
   schema, with apply and revert.
4. **Data assets, data tables and gameplay tags.** Tables are typed RON with
   row types in a shared crate, edited in a grid, imported from and exported
   to CSV. At deploy they are published as rows into SpacetimeDB through an
   owner-only reducer with a content hash; the same source is cooked for
   offline play. The M7 ability roster moves into a table. The publish
   reducer and the roster move touch `server/`, so they go in as a hot-path
   PR agreed with the Server track.
5. **Terrain brushes**, after the terrain runtime in M16: sculpt and paint
   modes, brush size, falloff and strength, weight layers.
6. **The rest of old Task 54:** undo for the remaining editor operations,
   multi-select and box select, global search, remaining preferences.
7. **Surface upgrades from `docs/EDITOR-UX.md`:** outliner folders with
   visibility and lock columns, content-browser filter chips, a snapping bar
   in the viewport, Eject, a command palette that covers assets and
   entities, tooltips and F1 pages generated from the schema.

**Demo:** a replay script edits a component, overrides a prefab field,
changes a table row and publishes it, with a golden image at each step.

**Gate:**
- Every registered component round-trips through the schema, and for every
  one of them an Inspector edit, add and remove is one undo entry that
  restores identical bytes.
- A component added in `game_shared` appears in Add Component with no editor
  change.
- Prefabs nest three deep; editing a base prefab updates 100 instances and
  keeps their overrides.
- A 10,000-row table lays out only the visible rows; CSV export then import
  is lossless; publishing unchanged data is a no-op by hash; changing an
  ability cooldown needs no module republish.
- A scripted brush stroke matches its golden heightfield, undo restores
  identical bytes, and only the touched cells re-cook.
- Every surface in `docs/EDITOR-UX.md` that this milestone touches has a
  golden image.

## M19: Offline slice

**Track:** Editor/Data. **Depends on:** M16 and M18. Carries old Tasks 42,
43, 55 and 56, and the gameplay node API of old Task 45.

**Goal:** a small offline game that can be played from start menu to a saved
and reloaded world, with no server.

**Scope notes:**
- Saves are local, versioned files written from the ECS and keyed by stable
  ids, as a difference against the cooked world. `game_shared` runs
  in-process. No bundled database.
- Navigation: a navmesh baked per cell and streamed with it, path queries
  and agents. The query side lives in `game_shared` and passes the wasm32
  check, because AI runs on the server in multiplayer.
- Game UI: a `.ui` asset on `crusty-gui` with a designer tab (hierarchy,
  canvas, style inspector, anchors), data binding, and the loading screen
  and fade that scene transitions need.
- Gameplay nodes: component get and set, physics queries, spawn and despawn,
  sound, the animation parameter bridge, timers; `event_input_action` gains
  Released, Held and an analog value with a migration; a CI check that
  server-realm nodes have no engine dependency.
- The animation content ledger closes here: jump, fall, land and cast clips,
  strafing.

**Demo:** a replay script plays the slice on lavapipe: menu, new game, walk,
an NPC follows a path, save, quit, load.

**Gate:**
- After save, quit and load, every entity with a stable id equals its saved
  state, and saving again produces an identical file.
- A save written by format version 1 loads under version 2 through a
  migration test.
- 200 agents path across streamed cells with no failed query; the navigation
  crate passes the wasm32 check.
- The HUD, the menu and the loading screen are `.ui` assets with no layout
  code in Rust.
- The slice's gameplay hooks are graphs, with no new Rust.

## M20: Global illumination

**Track:** Render/World. **Depends on:** M16. Carries the lighting half of
old Task 49.

**Goal:** indirect light that follows time of day in a streamed world.

**Decided now:** the technique is irradiance probe volumes. Hardware ray
tracing is used only to update probes faster, behind a player setting.
Image-based lighting from the sky and reflection probes land here too.

**First task, a spike:** choose how probes are updated, on an indoor and an
outdoor fixture. The candidates are probes captured at cook time and relit
at run time, baked probes per time-of-day keyframe streamed with cells, and
dynamic compute updates.

**Gate, fixed before the spike runs:**
- Tier 2, at 1080p on the RTX 5080: probe update and sampling together at
  most 2.0 ms.
- Probe memory at most 256 MB on the GPU for the loaded region, inside the
  3 GB cap.
- Indoor fixture (a closed room, sun outside): mean interior irradiance at
  most 2% of the exterior's, and no interior pixel above 5%.
- Outdoor fixture: total indirect energy within 10% of a converged
  high-density reference.
- Streaming: with the camera stopped, a cell arriving changes mean frame
  luminance by at most 2%.
- The floor-preset result is recorded as a diagnostic.

---

## Old task numbers

| Old task | New home |
|---|---|
| 39.5 Asset and systems editor windows | Closed as an open item; remaining window types arrive with the asset editors in M18 |
| 42 Save/load | M19 |
| 43 Scene transitions | M19, with the game UI |
| 44 Asset cooking and streaming | M16 |
| 45 Visual scripting tail | Runtime fix → M18.1; gameplay node API → M19; editor polish → parked with 45.5 |
| 45.5 Node graph editor v2 | Parked |
| 46 Terrain | Runtime → M16; brushes → M18 |
| 47 Sky and atmosphere | M15 |
| 48 Cascaded shadows | M15 |
| 49 Lighting, IBL and GI | Point and spot lights with culling → M15; image-based lighting and GI → M20 |
| 50 Material editor | Unscheduled until bindless materials (M14) and shader cooking (M16) exist |
| 51 VFX graph | Parked |
| 52 Prefab variants | M18 |
| 53 Audio graph | Parked |
| 54-A Component schema and Inspector undo | M18.2 |
| 54 Editor workflow polish | M18 |
| 55 In-game UI framework | M19 |
| 56 Navigation and AI | M19 |
| 57 Behaviour tree editor | Parked |
| 58 Parallel ECS | Early half → M12.6; the parallel executor when a profile asks for it |
| 58.5 Multi-window viewport | Parked |
| 60 Production hardening and ship a game | Retired as an umbrella. The proof is the two slices: MMO in M17, offline in M19 |

## Parked

Not scheduled. Each keeps its old number. The full original text is in git
history (`git show 6b9e008:docs/roadmap/ROADMAP.md`).

- **45.5 Node graph editor v2.** The backlog is the deferred ledger in
  `docs/mockup/AUDIT.md` and `docs/mockup/AUDIT-2026-08.md`. Start by
  splitting `graph_editor_crusty.rs` (15,690 lines) into draw, interaction
  and menu modules. Also here: array constants on canvas, an expression
  node, separate warning and error badges, per-scope bookmarks.
- **51 VFX graph.** Compiles a particle graph to the compute kernel of the
  existing particle system. Needs the material editor's code generation
  first.
- **53 Audio graph.** A graph compiled to a Kira effect chain; parameters
  follow the animation-graph pattern.
- **57 Behaviour tree editor.** A graph domain over the M19 navigation and
  AI runtime; the tree must compile in `game_shared`.
- **58.5 Multi-window viewport.** A viewport in its own OS window needs a
  second swapchain and touches the render-thread contract.

## Unscheduled

Wanted, with no milestone yet.

- **Virtual geometry and virtual texturing.** Last in the rendering order;
  they pay off only once everything before them exists.
- **50 Material editor.** Its generated code must target the bindless
  material layout, and packaged builds need cooked shaders.
- **Parallel system executor.** The second half of old Task 58.
- **Server-side NPCs at scale.** After M17 and M19.
- **Texture streaming.** After cooking in M16.
- **Hardening:** a crash handler with a report file, fallbacks for failed
  asset loads.
- **Lua scripting, a UI graph, motion matching, graphs running inside the
  SpacetimeDB module.** Only when a concrete need appears. The CI check that
  keeps server-side graph execution possible stays.

## Open follow-ups

Deferred by finished tasks; the full ledgers are in COMPLETED.md.

| Follow-up | Home |
|---|---|
| Inspector edits bypass undo | M18.2 |
| Plugin-staged resources reach only the startup world | M18.1 |
| `event_input_action` has only a Pressed output | M19 |
| `GraphScriptRunnerSystem` understates its writes | M12.6 |
| Vulkan validation layers are never enabled | M11.2 |
| Animation content: jump, fall, land and cast clips, strafing, FK/IK split, pelvis-drop cap, additive layers | M19 |
| Server ceiling of about 150 movers | M13 |
| Graph plan cache is invalidated wholesale | M19, with the gameplay nodes |
| `physics_rapier` cannot be stripped from exports | M18.2 |
| `game_client/src/app.rs` is 9,514 lines | Split when a milestone next changes it substantially |
| The Rust toolchain is pinned to 1.100.1 in `rust-toolchain.toml` | Upgrade in a PR of its own that also fixes the new lints |
| The colour picker's pipette can be armed on Linux, where nothing samples the desktop, so it never picks (Escape or right-click still cancels) | M18 |

---

## Settled decisions

The twelve decisions of the 2026-11 audit (keep Vulkano, `hecs` and
SpacetimeDB; rendering order; terrain; saves; data tables; editor references;
authority) are recorded in
[`docs/adr/0003-2026-11-audit-settled-decisions.md`](../adr/0003-2026-11-audit-settled-decisions.md).
Reopen one only with new evidence. Earlier decisions are in `docs/DECISIONS.md`.

---

**Last updated:** 2026-11-28. Rewritten around the two target games after
the 2026-11 audit: milestones M11–M20 replace Tasks 42–60.
