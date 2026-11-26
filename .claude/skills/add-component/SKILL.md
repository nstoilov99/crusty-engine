---
name: add-component
description: Add or change an ECS component end to end — definition, RON scene serialization, Inspector section and Add Component entry, clipboard/GUID remap, play-mode snapshot, tests. Use when a task introduces or extends a component.
---

There is no dynamic component registry yet (Task 54-A proposes one): a
component exists in five places and a scene round-trip test proves them
consistent. Exemplar: commit `2855d9a` (inspector sections for
`CharacterMovement`, `PlayerInput`, `OrbitCamera`).

## Steps

1. **Define** it as plain data (`Debug, Clone, Serialize, Deserialize`,
   usually `Default` + `#[serde(default)]`; transient fields `#[serde(skip)]`).
   Engine components: `engine/src/engine/ecs/components.rs`. Gameplay
   components shared with the server: `game_shared/src/components.rs` (must
   stay engine-free; `game_shared` is wasm32-checked). No methods with
   behaviour — logic goes in a system.
2. **Serialize.** `ComponentData` in `engine/src/engine/scene/scene_format.rs`
   is the schema: add a variant, then both directions in
   `engine/src/engine/scene/scene_serializer.rs` (`serialize_entity`,
   `spawn_entity_from_data`). Prefabs: `scene/prefab.rs` if they may carry it.
   Scene files have no version-dispatch migration — compatibility is serde
   defaults / aliases; a rename needs an alias or an update of every
   committed `.scene`.
3. **Inspector.** `engine/src/engine/editor/inspector_panel.rs`: allocate a
   `ComponentPresence` bit and probe it. `inspector_crusty.rs`: an `edit_*`
   section called from `render_components`, a `ComponentAction` removal
   variant, and a guarded `Default` insertion in `render_add_component`;
   clamp ranges there. Note: Inspector edits do **not** record undo today
   (`InspectorPanelCtx` has no `CommandHistory`) — do not claim undo coverage.
4. **GUID fields.** Paste regenerates entity GUIDs and remaps only
   `Parent.parent_guid`; a component holding an entity GUID needs its own
   remap in the subtree paste path or the reference dangles.
5. **Play mode** uses the same serializer for snapshot/restore
   (`editor/play_mode.rs`), so step 2 is what makes Play/Stop preserve it;
   `#[serde(skip)]` fields reset on restore by design.
6. **Content.** Update committed scenes that should carry the component
   (`content/scenes/*.scene`) and re-save through the editor rather than by
   hand when the format is non-trivial.

## Tests

- Serde round-trip and `Default` for the type.
- `scene_format::gameplay_components_roundtrip_config_only` style test and
  `engine/tests/serialization_roundtrip.rs`.
- `inspector_panel::presence_probe_*` for the new bit.
- `engine/tests/play_mode.rs` if play-mode behaviour depends on it.
- Clipboard: `paste_remaps_intra_subtree_parent_guids` pattern for any GUID
  field.

Done when a scene containing the component saves, reloads, copies/pastes and
survives Play → Stop byte-identically, and the Inspector adds, edits and
removes it.
