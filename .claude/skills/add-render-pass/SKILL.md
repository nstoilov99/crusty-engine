---
name: add-render-pass
description: Add a render pass to the deferred pipeline — DeferredPass resize/rebind, passes_mut registration, add_pass_with graph declaration, PassPipelineBuilder, shaders, FramePacket inputs, verification. Use when adding or restructuring a pass (sky, fog, CSM, post-process).
---

A pass touches three sites (pass file, constructor + `passes_mut()`, one
`add_pass_with` block). Exemplar: SSAO — `ssao_pass.rs`, `ssao.frag`,
`ssao_blur.frag`, shared `composite.vert`. All under
`engine/src/engine/rendering/3d/deferred/` and `rendering/shaders/deferred/`.

## Rules that are not negotiable

- Game space is Z-up; convert once, before packing into `FramePacket`, with
  `engine/src/engine/adapters/render_adapter.rs` (`world_matrix_to_render`,
  `position_to_render`, `direction_to_render`). Shaders see Y-up only.
- The main thread does logic and UI layout; the render thread records and
  submits. Anything the pass needs per frame is an owned, `Send` field on
  `FramePacket` (`rendering/frame_packet.rs`), filled on the main thread and
  consumed in `RenderThread::thread_entry`.
- Scene, preview and UI command buffers stay one chained `then_execute`
  sequence (the `TargetRenderer` serial contract, `render_thread.rs`).

## Steps

1. **Pass struct** implementing `DeferredPass` (`deferred/pass.rs`): `name`,
   `resize(PassResizeContext)` recreates only pass-owned size-dependent
   images/framebuffers, `rebind(PassInputs)` rebuilds descriptors that
   reference G-buffer, shared or cross-pass outputs. Never keep old views.
2. **Pipeline** via `PassPipelineBuilder` (`deferred/pass_pipeline.rs`):
   fullscreen passes are `new(...).build()`; geometry passes add
   `vertex_input`, `depth`, `blend`, `topology`, `rasterization` as needed.
3. **Shaders** as GLSL under `shaders/deferred/`, compiled at build time with
   `vulkano_shaders::shader!` modules (the `ShaderCompiler` shaderc path is
   editor-only hot reload, not how passes ship).
4. **Construct and register:** field on `DeferredRenderer`, built in
   `DeferredRenderer::new`; add to the `passes_mut()` array so `resize`
   recreates shared targets → resizes every pass → snapshots inputs →
   rebinds every pass.
5. **Graph declaration** in `DeferredRenderer::render`: `RenderGraph::add_pass_with`
   with `PassBuilder::{read,write,modify}` in the setup closure and matching
   `PassContext::{mark_read,mark_write}` before recording. Debug builds
   cross-check declared vs actual touches (`RenderGraph::check_touches`); an
   undeclared read is `RenderError::Graph`, not a silent pass.
6. **Errors** are the typed `RenderError` (`rendering/error.rs`:
   `PassResize`, `PassRebind`, `Graph`, `Render`); the render thread turns
   them into `RenderEvent::RenderError` with pass context.
7. **Settings** exposed to users go through `PostProcessingSettings` and the
   Inspector/settings window, not constants in the shader.

## Verify

- Debug build (graph touch checks run only there), editor and standalone.
- Resize repeatedly, minimise/restore, toggle the pass on/off: no
  stale-descriptor or layout errors. Validation layers are not enabled by
  default — enable `VK_LAYER_KHRONOS_validation` in
  `core/vulkan_context.rs` locally for the check.
- `cargo test -p rust_engine rendering` and the editor smoke run
  (`run-app` skill) — grep the log for `panic|validation`.

Done when the pass renders in both hosts, resize is clean under validation
layers, and the three-site rule held (no new `match pass_name` strings).
