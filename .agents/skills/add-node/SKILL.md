---
name: add-node
description: Add a node type to the node-graph system (script graph pure/impure node, new Effect, descriptor migration) across the portable crates and the engine runner. Use when adding, versioning or wiring a graph node.
---

Nodes are declared in portable crates and executed through `Effect`s; the
engine never lets a node mutate the world directly. Every step below has a
test that fails until it is done.

## Where things live

| Concern | File |
|---|---|
| Descriptor schema, registry, validation | `crates/node_graph_types/src/registry.rs` (`NodeDescriptor`, `PinDescriptor`, `NodeRegistry::register`) |
| Standard library descriptors | `crates/node_graph_types/src/std_nodes.rs` (`logic_math()`, `effects()`, `std_node_descriptors()`) |
| Dev/demo fixtures (descriptors only) | `crates/node_graph_types/src/dev_nodes.rs`, engine plugin `engine/src/engine/plugins/dev_nodes.rs` |
| Node behaviour | `crates/node_graph_exec/src/nodes.rs` (`PureNode::eval`, `ImpureNode::fire`, `register_impls()`) |
| Effects | `crates/node_graph_exec/src/effect.rs` (`Effect` enum); applied in `engine/src/engine/scripting/runner.rs` (`GraphScriptRunnerSystem::apply`) |
| Document validation / migrations | `crates/node_graph_types/src/validate.rs`, `migrate.rs` (`MigrationCtx`, `NodeRegistry::register_migration`) |
| Derive macros (optional) | `crates/node_graph_macros/src/lib.rs` (`#[derive(ScriptNode)]`, `#[node(...)]`, `#[input]/#[output]`); inventory registration via `engine/src/engine/node_graph/auto_register.rs` |
| Engine registration | `engine/src/engine/plugins/graph_scripting.rs` (`GraphScriptingPlugin::build`) |

## Steps

1. **Descriptor.** Add an ID constant and a `NodeDescriptor` to the right
   group in `std_nodes.rs`: immutable `id` and pin slugs, `version`,
   `NodeRealm`, `pure`, `deterministic`, defaults, `doc` on the node and
   `with_doc` on every pin. The registry rejects pure nodes with Exec pins
   and impure nodes without Exec flow — a pure node has no exec pins.
2. **Behaviour.** Implement `PureNode` (read inputs with the `EvalCtx`
   accessors, `set_output`) or `ImpureNode` (`fire` emits through
   `FireCtx::emit`, returns `Continue(EXEC_OUT_PIN)`), then add the same ID
   to `register_impls()`. `NodeImpls::check_plan` fails on a missing or
   purity-mismatched implementation.
3. **New world action?** Add an `Effect` variant and its arm in
   `GraphScriptRunnerSystem::apply`. If the runner now writes a component or
   resource it did not before, add it to the system's declared access.
4. **Changing a shipped descriptor** (pins, defaults, outputs): bump
   `version`, register a migration for each step, and add a fixture under the
   migration tests. Saved graphs reference the old shape.
5. **Portability.** `node_graph_types` and `node_graph_exec` stay free of
   engine, ECS, render and clock dependencies — inputs come through
   `TickInput` / `WorldRead`, outputs through `Effect`. CI checks both
   standalone and for `wasm32-unknown-unknown`.
6. **Tests to extend:** `std_nodes.rs` (descriptor present, docs non-empty),
   `crates/node_graph_exec/tests/std_library.rs` (behaviour),
   `walking_skeleton.rs` if the node joins the demo path,
   `engine/src/engine/scripting/acceptance.rs` for a new Effect; macro users
   add to `engine/src/engine/node_graph/macros_test.rs`.
7. **Exercise it** in `content/graphs/runner_demo.graph` or a test graph and
   confirm it appears in the editor palette with its doc tooltip.

Done when: `cargo test -p node_graph_types -p node_graph_exec` and
`cargo test -p rust_engine scripting` pass, the wasm32 check passes, and the
node's `doc` reads as a complete sentence in the palette.

Anim-graph nodes follow the same registry pattern but live in
`engine/src/engine/animation/graph/` with their own registries
(`anim_node_registry` is the union for validation; palettes use subsets).
