# M7 — Net-C: Authoritative Combat & Thin HUD

**Status:** ✅ Approved (2026-07-19) — gpt-5.6-Sol/Codex plan review
reconciled, 8 findings folded in; open questions ruled (all defaults).

Adds the first combat loop on top of M6's authoritative movement: a
server-authoritative ability system (GCD, cooldowns, resources, range/LoS/
target legality, cast times with interrupts), combat resolution as reducer
transactions (damage, death, respawn via spawn generations), an exploit-test
battery, and a thin in-game HUD in crusty-gui (action bar, cast bar, target
frame, connection indicator). Consumes the M6 D6 combat groundwork
(`Broadphase`, `hitscan`, `projectile_step`).

## Current state (verified 2026-07-19)

- **No combat concepts exist anywhere.** Repo-wide grep for
  health/damage/death/respawn/ability/mana/cooldown: zero matches. `Player`
  and `Npc` rows have no hp; `EntityState` carries only pos/vel/yaw/zone.
- Server (`server/game_module/src/lib.rs`): 50 ms `move_tick` consumes the
  `PendingInput` queue (≤ `MAX_STEPS_PER_TICK = 4` per player) through
  `motion::step`; 100 ms `tick` does clock, tombstone GC, and inline NPC
  wander (NPCs never call `motion::step`, no Z, no collision). `submit_input`
  validates session + `accept_input` (epoch match, seq strictly increasing) +
  finite floats; queue depth cap 8.
- Generations: `Player` and `Npc` **already carry `generation: u32`** (seeded
  to 1), and `derive_proxy_guid(realm, entity_id, generation)` /
  `Tombstone { entity_id, generation }` exist — but **nothing bumps a
  generation today** (M5 note: "entity ids are never reused"). Client-side
  generation-replace is already unit-tested
  (`generation_change_on_live_row_is_replace`).
- **NPCs spawn underground**: `seed_npcs` places them at `SPAWN_POINT[2] = 1`
  while the greybox surface at the origin is z ≈ 8 (players spawn at capsule
  center z = 9). Harmless while NPCs are decorative; fatal once 3D range/LoS
  checks exist — they'd be unhittable. Must be fixed alongside combat.
- M6 D6 groundwork in `game_shared::motion`: `Broadphase` (64 m `world_grid`
  cells, id-sorted candidates), `hitscan` (world raycast + capsule tests,
  deterministic tie-breaks), `projectile_step` (semi-implicit gravity +
  segment sweep). **Server does not yet build a `Broadphase` per tick** — the
  groundwork is shared code + tests only.
- Offline player rows persist with `session: None` (positions survive
  disconnect by design) — target legality must exclude session-less players.
- Epoch restarts are already the prediction-reset mechanism (client handles
  them end to end) — respawn can reuse this instead of inventing a new reset
  channel.
- crusty-gui is **editor-only**: `dep:crusty-gui` is optional, enabled solely
  by the `editor` feature; `FramePacket.crusty_paint` and the render-thread
  crusty pass are `#[cfg(feature = "editor")]`; the standalone branch renders
  zero UI (status lives in the window title). crusty-gui is a single flat
  crate — the minimal embedding path is `Context` → `run_root` →
  `FrameOutput.paint` → `TargetRenderer::render`, no dock/shell needed.
- Standalone now streams the greybox world (fix `bf9ca38`), so client and
  server share one world; combat happens on that content.

### Binding constraints

- M0 ceilings: serial reducer ceiling ~1,100–1,200/s; one delivery flush per
  transaction. Cast resolution piggybacks on existing scheduled transactions
  where possible; client-called reducers stay O(1) per call.
- ADR-015 lineage: all geometry queries go through `game_shared::collision` /
  `motion` primitives — no new collision math, no Rapier in the module.
- Combat math must be shared-code pure functions (`game_shared::combat`) so
  the HUD can render authoritative-consistent state (cooldown end times, cast
  progress) without asking the server, and so validation is unit-testable
  outside the module.
- Schema migration policy unchanged: every schema-touching package requires
  `./publish.ps1 -Wipe`; timer inserts stay idempotent.
- Times are server micros (`ctx.timestamp`), matching `last_update_micros` and
  the M5 NetClock offset the client already maintains — the HUD converts
  server end-times to local time via the existing clock sync.
- CLAUDE.md: any new crusty-gui API lands in `../crusty-gui` first; both repos
  must build before commit.

## Design

### D1. Shared ability model (`game_shared::combat`)

Static const ability roster — not a DB table (no live tuning need in M7; both
sides need identical data at compile time; a table adds seeding + subscription
cost for nothing).

- `AbilityId` (u16 newtype) + `AbilityDef { id, name, cast_secs, cooldown_secs,
  mana_cost, range_m, kind }` in a `pub const ABILITIES: &[AbilityDef]` slice.
- `AbilityKind`: `Strike` (instant hitscan), `Projectile` (cast then launch),
  `NovaAoe` (instant, radius around caster), `Heal` (cast, self-target).
- Slice roster (one per resolution path, tuning numbers are package-1
  constants, not contracts):
  1. `strike` — instant, range 3 m, 0 mana, no cooldown (GCD-limited), 15 dmg.
  2. `firebolt` — 1.5 s cast, 25 m range, 30 mana, 20 m/s projectile, 25 dmg.
  3. `nova` — instant, 8 m radius AoE, 40 mana, 8 s cooldown, 15 dmg.
  4. `heal` — 2 s cast, self-only, 35 mana, 5 s cooldown, +30 hp.
- `GCD_SECS = 1.0`, `RESPAWN_SECS = 5.0`, `MANA_REGEN_PER_SEC`, hp/mana caps.
- Pure validation helpers with unit tests (no ECS, no tables):
  `can_cast(def, now, gcd_until, cooldown_until, mana, alive) -> Result<(), CastError>`
  and `target_legal(kind, caster, target) -> Result<(), CastError>`;
  `CastError` is a small enum the reducer maps to its error string, and the
  exploit tests assert on.

### D2. Combat state schema

- `Player` gains: `hp: f32, hp_max: f32, mana: f32, mana_max: f32,
  alive: bool, respawn_at_micros: u64, gcd_until_micros: u64`
  (`generation` already exists).
- `Npc` gains: `hp: f32, hp_max: f32, alive: bool, respawn_at_micros: u64`
  (`generation` already exists). Dead NPC rows are retained (like players)
  until `tick` respawns them. Package 1 also fixes the NPC spawn Z: place
  them on the greybox surface near the player spawn (z ≈ 8 + capsule
  half-height), not the stale z = 1.
- New public table `AbilityCooldown { key: u64 PK, entity_id: u64 btree,
  ability_id: u16, ready_at_micros: u64 }` with a **deterministic key**
  `key = entity_id << 16 | ability_id` (allocator ids stay far below 2^48) —
  upsert-by-PK is race-free by construction, no auto_inc + scan-for-duplicate
  dance. GC'd when expired during `tick`. Subscription: own rows only
  (`WHERE entity_id = <own>`) feeds the HUD sweeps; end-times, not remaining
  durations, so replication latency doesn't skew the sweep.
- New public table `ActiveCast { entity_id: u64 PK, ability_id: u16,
  target_entity_id: u64, zone_id: u32 btree, start_micros: u64,
  finish_micros: u64 }` — public so the target frame can show the target's
  cast bar too; at most one per entity (new cast replaces). `zone_id` is
  copied from the caster row at cast time so the row fits the **existing
  zone-scoped subscription pattern** (subscriptions are column predicates —
  "by caster zone" needs a real column; a mid-cast zone change is impossible
  since moving cancels the cast).
- `EntityState` (protocol) gains `hp: f32, hp_max: f32, alive: bool` so
  proxies/target frame get health without extra subscriptions. Replication
  treats hp as latest-value (never interpolated — it rides alongside the
  interp buffer, not through it). `PROTOCOL_VERSION` bumps once (package 1).

### D3. Cast pipeline (server)

New client-called reducer `cast_ability(ability_id: u16,
target_entity_id: u64)` (0 = no target / self):

- Validation order (first failure returns `Err` and aborts the transaction —
  the transaction failure IS the rejection; no partial state): session is the
  active one → caster alive → ability exists → `can_cast` (GCD + cooldown +
  mana) → target legality (`target_legal`: exists as Player-with-live-session
  or Npc, alive, self only where the kind allows it, not-self for hostile) →
  range (3D distance between server-row positions ≤ `range_m`) → LoS
  (`ChunkStore::raycast` eye-to-eye against the embedded store — world blocks,
  entities don't; eye = capsule center + fixed offset constant).
- Instant kinds resolve in the same transaction: spend mana, start GCD, set
  cooldown row, apply damage/heal (D5). `strike` re-checks the hit through
  `hitscan` with a single-candidate `Broadphase` (world can still block the
  hit); `nova` builds a `Broadphase` from live players + NPCs in the
  overlapped cells and applies `aoe_candidates` + a true radius filter.
- Cast kinds insert/replace `ActiveCast`, spend nothing yet (mana + cooldown
  commit at completion — cancel loses nothing but the GCD), start GCD
  immediately (prevents instant-weaving during a cast).
- **Completion runs inside `move_tick`** (already iterating everything at
  20 Hz; 50 ms granularity is fine for 1.5–2 s casts; no per-cast timer rows,
  respecting the M0 transaction budget): for each `ActiveCast` with
  `finish_micros ≤ now` — re-validate target alive + range + LoS (things
  moved), then spend mana, set cooldown, resolve (Heal applies; Projectile
  spawns a `Projectile` row, D4).
- **Ordering inside `move_tick` is normative**: consume movement inputs and
  apply cast interruptions **first**, resolve due casts **second**. Both
  happen in one serial transaction, so a cast finishing on the same tick as
  a queued move would otherwise land-or-cancel purely by implementation
  accident; cancel-before-complete makes movement always win the tie.
- Interruption rules: any consumed movement input with nonzero `move_dir` or
  `jump` cancels the caster's `ActiveCast` (checked in `move_tick` where
  inputs are already popped); death cancels; a new `cast_ability` replaces;
  disconnect teardown deletes it. Damage pushback: deferred (non-goal).
- Mana regen: `tick` regenerates `MANA_REGEN_PER_SEC × elapsed`, where
  elapsed is measured from the row's last regen timestamp (scheduled ticks
  can run late — a fixed ×0.1 would silently lose regen time); clamped to
  `mana_max`. Same place GC's expired `AbilityCooldown` rows.

### D4. Projectile representation (cut-line candidate)

M6 deferred this decision to M7. Decision: **a public `Projectile` table**,
stepped server-side, replicated like any row — not a fire-and-forget visual
event (would desync with server hit results), not a new `EntityState` kind
woven through the interp pipeline (projectiles are short-lived and fast;
linear client-side extrapolation from spawn state looks better than delayed
interpolation).

- `Projectile { entity_id: u64 PK, caster_entity_id: u64, ability_id: u16,
  x y z vx vy vz: f32, zone_id: u32 btree, spawned_at_micros: u64,
  last_update_micros: u64 }`, entity ids from the existing allocator.
  `zone_id` set at spawn (3 s lifetime — no re-zoning) so the row fits the
  existing column-predicate zone subscriptions.
- Stepped in `move_tick` via `motion::projectile_step` (50 ms, gravity
  constant from `MotionConfig`), against a `Broadphase` built once per tick
  from live players + NPCs (the same one nova uses — built at most once per
  transaction, reused); each step writes pos/vel + `last_update_micros`.
  Hit → apply damage (D5), delete row. Miss beyond a lifetime cap (3 s) →
  delete. Caster is excluded from its own projectile's candidates for the
  first 2 ticks (muzzle overlap).
- Client renders projectile rows as small unlit spheres, extrapolated locally
  from the **last server step, not spawn** (the server rewrites pos each
  tick — extrapolating from `spawned_at` would double-count the whole flight):
  `dt = local_server_now − last_update_micros`, then
  `pos + vel·dt + ½·g·dt²` with the same `MotionConfig` gravity the server
  integrates (dt ≤ ~1 tick + latency, so drift is negligible). Rows vanish
  on hit — impact VFX are a non-goal.
- Cut line: if the milestone runs long, `firebolt` becomes hitscan-at-
  completion and this package is dropped whole — nothing else depends on it.

### D5. Damage, death, respawn (reducer transactions)

- `apply_damage(target, amount)` / heal are plain module functions inside the
  resolving transaction — hp clamps to [0, hp_max]; no damage events table in
  M7 (HUD derives feedback from hp deltas).
- **Player death** (hp → 0): `alive = false`, `respawn_at_micros = now +
  RESPAWN_SECS`, velocity zeroed, `PendingInput` purged, `ActiveCast`
  removed, `Tombstone { entity_id, generation }` upserted. `submit_input` and
  `cast_ability` reject while dead. `move_tick` skips dead players (no
  gravity for corpses — the row is inert until respawn).
- **Player respawn** (checked in `move_tick`): teleport to the spawn point,
  full hp/mana, `alive = true`, `generation += 1`, **`epoch += 1`** — the
  epoch bump makes the client's existing prediction-restart path handle the
  respawn snap for free; the generation bump + tombstone make every remote
  client destroy-and-respawn the proxy through the already-tested
  generation-replace path. This is the "respawn via spawn generations" the
  roadmap names, and it finally exercises that M5 client code against a real
  server path.
- **NPC death/respawn**: same tombstone + generation bump; `tick` respawns
  dead NPCs at their wander anchor after `RESPAWN_SECS` with a fresh
  generation. NPC rows keep their entity id (allocator ids are never reused;
  generation is the incarnation counter).
- Client: local death disables prediction + input send (driven by own-row
  `alive`), shows the respawn state in the HUD; the epoch bump on respawn
  re-seeds prediction exactly like a reconnect.

### D6. Client combat state & exploit battery

- `game_client_net` additions: `cast_ability(ability_id, target)` call-through;
  own-combat getter `own_combat() -> Option<OwnCombat>` (hp/mana/gcd_until/
  alive + per-ability `ready_at` + active cast) polled per frame — no new
  `NetEvent` variants; polling a struct is simpler than event-threading state
  the HUD re-reads every frame anyway. Proxy hp arrives via the extended
  `EntityState` and lands on replication's proxy bookkeeping (latest-value).
- Targeting is client-side selection state (Tab cycles visible proxies
  nearest-first, sent per `cast_ability` call); the server never stores a
  target — legality is re-checked per cast/completion against the argument.
- **Exploit acceptance tests** (extend `game_client_net/tests/acceptance.rs`,
  same live-standalone infra; assertions are effect-based — observe hp/mana/
  cooldown rows, since reducer errors don't surface to the caller cleanly):
  - cooldown bypass: two immediate `strike` casts → exactly one hp delta;
  - out-of-range cast → zero hp delta;
  - dead caster / dead target / session-less (offline) target → rejected;
  - mana bypass: spam `nova` until mana floor → casts stop landing;
  - movement input mid-cast → `ActiveCast` gone, no resolution;
  - replayed/reordered `submit_input` (forged seq/epoch) → position
    unaffected (tightens the M6-era guarantee into an explicit test);
  - forged-identity casting is structurally impossible (reducers act on
    `ctx.sender`) — documented, not tested.

### D7. Thin HUD (crusty-gui in standalone)

- New cargo feature `hud` on `engine` and `game_client` (default for
  `game_client`), enabling `dep:crusty-gui` independently of `editor`.
  **Honest scope** (this is the widest seam in the milestone, not a two-line
  gate tweak): the *entire* `gui` module is `#[cfg(feature = "editor")]` in
  `engine/mod.rs`, `CrustyGui` imports `EditorTheme` from the editor module,
  and the editor-gated sites span `frame_packet.rs` (~20 gates),
  `render_thread.rs` (~25 gates incl. `CrustyRenderer` init), and the
  standalone submission branch, which today **never chains a GUI command
  buffer**. The work: gate `gui` on `any(editor, hud)`, split a runtime HUD
  core out of `CrustyGui` (theme-free `Style` constructor — lands in
  `../crusty-gui` first per convention; `EditorTheme` wiring stays
  editor-gated), widen the `crusty_paint`/`crusty_text` packet+config gates
  to `any(editor, hud)`, and add the composite-after-deferred crusty CB to
  the standalone submission branch (the `TargetRenderer` serial-execution
  contract already holds there — one CB chain per frame).
- `StandaloneApp` owns the runtime HUD context and lays out the HUD each
  frame from `own_combat()` + targeting state; paint list rides the packet.
- Widgets (all existing crusty primitives — rects, text, `Painter` polygons;
  no new widget framework, that stays deferred per Task 55):
  - **Action bar**: 4 slots (keys 1–4), radial cooldown sweep from
    `ready_at − now` over `cooldown_secs`, GCD dimming, mana-insufficient
    tint;
  - **Cast bar**: progress `now − start` over `finish − start`, ability name;
  - **Target frame**: target label + hp bar + its cast bar if any. Label is
    a short derived tag ("Player a1b2…" / "Dummy #3") — `EntityState` carries
    no display name and name plumbing is out of scope;
  - **Own frame**: hp/mana bars, dead/respawn countdown state;
  - **Connection indicator**: the `status_line()` content moves from the
    window title into a corner chip (title keeps only the app name).
- All times are server-micros converted through `NetClock`. The clock's EWMA
  offset can step estimated server time *backwards* between frames, so the
  HUD keeps a per-frame `server_now = max(estimate, previous)` — sweeps and
  cast bars are monotonic by construction, and remaining-time clamps at 0.

### Non-goals (deferred)

- Aggro, threat, NPC abilities, AI (Task 56); NPCs stay wandering dummies.
- Damage pushback on cast, buffs/debuffs/DoTs, XP/loot, floating combat text,
  impact VFX/audio, ability tooltips.
- General in-game UI framework (old Task 55 remainder), mouse-pick targeting.
- Interest-managed/layered subscriptions (M8) — combat rows ride the existing
  zone-scoped subscription set.
- PvP rules/factions: everything can hit everything (except self) in M7.
- Live ability tuning (const roster; changes are a republish).

## Work packages (each = one reviewable commit)

1. **Combat schema + shared model** — `game_shared::combat` (roster, consts,
   `can_cast`/`target_legal` + unit tests), Player/Npc combat columns
   (`generation` already exists), NPC spawn-Z fix onto the greybox surface,
   `AbilityCooldown`/`ActiveCast` tables, `EntityState` hp fields, protocol
   version bump, bindings regen, client compiles with latest-value hp
   plumbed to proxies. Requires `-Wipe` publish.
2. **Cast pipeline** — `cast_ability` reducer (full validation chain, LoS,
   instant resolution incl. nova's per-transaction `Broadphase`), completion
   + interruption in `move_tick`, mana regen + cooldown GC in `tick`.
   CLI-testable via `spacetime call` before any client UI exists.
3. **Death & respawn** — death state + input/cast purge, tombstone +
   generation bump, epoch-bump respawn, NPC death/respawn in `tick`, client
   prediction gating on `alive`, acceptance test: kill NPC → tombstone +
   fresh-generation respawn; die → respawn snaps cleanly. `-Wipe` publish.
4. **Projectile** *(cut-line)* — `Projectile` table, `move_tick` stepping via
   `projectile_step`, shared per-tick `Broadphase`, client extrapolated
   rendering. `-Wipe` publish.
5. **Client combat + exploit battery** — `cast_ability`/`own_combat` in
   `game_client_net`, Tab targeting, D6's acceptance exploit tests green
   against a fresh publish.
6. **HUD seam** — `hud` feature, `gui` module regated to `any(editor, hud)`
   with the runtime HUD core split out (theme wiring stays editor-only),
   widened packet/config gates, standalone submission branch chains the
   crusty CB, crusty-gui `Style` constructor (in `../crusty-gui` first),
   empty HUD frame showing only the connection chip (title cleanup).
   Both repos + all three build configs (standalone, editor, hud-less) green.
7. **HUD widgets** — action bar with sweeps, cast/own/target frames, dead
   state; two-instance manual test + screenshot review pass (per the visual-
   change workflow).

## Risks & mitigations

| Risk | Mitigation |
|---|---|
| `move_tick` transaction growth (casts + projectiles + respawns on top of movement) | everything bounded per-entity; one shared `Broadphase` per transaction; keep the p95/p99 timers from M6 pkg 5 and re-measure with combat load |
| `cast_ability` reducer spam (client-called, unscheduled) | validation is cheap early-out (GCD check first); M0 call ceiling documents the bound; no per-call allocation-heavy work before validation passes |
| Reducer `Err` visibility (client can't read rejection reasons) | exploit tests assert effects, not messages; `CastError` strings still logged server-side for debugging |
| hp through the interp pipeline causing ghost values | hp is latest-value by construction (D2); test that a dead proxy never shows stale positive hp |
| Respawn epoch bump interacting with in-flight inputs | inputs are epoch-gated already (`accept_input`); stale-epoch inputs after respawn are dropped by design — acceptance test covers it |
| `hud` feature fracturing build configs | package 6 explicitly builds all three configs; CI-equivalent check is part of its acceptance |
| D7 seam wider than expected (gui module split, ~45 editor gates, standalone CB chaining) | package 6 is seam-only (no widgets) so the split lands reviewable; crusty-gui side stays one small addition (theme-free `Style`) — anything larger there is a red flag to stop and re-scope |
| Cast completion at 50 ms granularity feels off | completion timing error ≤ 50 ms on 1.5–2 s casts (≤ 3%); HUD cast bar uses its own clock so the visual finishes exactly on time |

## Decisions (ruled 2026-07-19, all defaults accepted)

1. **Package 4 (projectile): in** — exercises the M6 groundwork end to end;
   remains the designated cut if the milestone runs long.
2. **`hud` is a default feature of `game_client`** — standalone without a
   HUD stops making sense after this milestone.
3. **Heal is self-only** — ally targeting adds friendly-legality rules for
   zero slice value.
4. **Everything-hits-everything PvP** — no factions; two players CAN kill
   each other in the co-op slice.
