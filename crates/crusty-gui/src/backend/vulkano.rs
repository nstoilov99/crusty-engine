//! Vulkano render backend for crusty-gui.
//!
//! Phase 1.5 layout:
//!
//! * [`OffscreenTargets`] owns three sampled color images sized to the
//!   swapchain — `scene` (where the host renders content the panels will
//!   blur over), and two blur ping-pong images. All share a single
//!   "offscreen" render pass.
//! * [`BlurRenderer`] runs a Kawase 4-tap blur from a source image view
//!   into a destination framebuffer.
//! * [`UiRenderer`] renders tessellated UI meshes. Every draw consumes a
//!   backdrop image view that the rrect-fill shader path samples to fake
//!   frosted glass. For passes that don't need a backdrop (e.g. drawing
//!   the scene itself) the caller passes `targets.dummy_view()`.

use std::collections::HashMap;
use std::sync::Arc;

use vulkano::buffer::allocator::{SubbufferAllocator, SubbufferAllocatorCreateInfo};
use vulkano::buffer::{Buffer, BufferContents, BufferCreateInfo, BufferUsage, Subbuffer};
use vulkano::command_buffer::allocator::StandardCommandBufferAllocator;
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, CommandBufferUsage, CopyBufferToImageInfo, PrimaryAutoCommandBuffer,
};
use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
use vulkano::descriptor_set::layout::{
    DescriptorSetLayout, DescriptorSetLayoutBinding, DescriptorSetLayoutCreateInfo, DescriptorType,
};
use vulkano::descriptor_set::{DescriptorSet, WriteDescriptorSet};
use vulkano::device::{Device, Queue};
use vulkano::format::Format;
use vulkano::image::sampler::{Filter, Sampler, SamplerAddressMode, SamplerCreateInfo, SamplerMipmapMode};
use vulkano::image::view::ImageView;
use vulkano::image::{Image, ImageCreateInfo, ImageType, ImageUsage};
use vulkano::memory::allocator::{
    AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator,
};
use vulkano::pipeline::graphics::GraphicsPipelineCreateInfo;
use vulkano::pipeline::graphics::color_blend::{
    AttachmentBlend, BlendFactor, BlendOp, ColorBlendAttachmentState, ColorBlendState,
};
use vulkano::pipeline::graphics::input_assembly::{InputAssemblyState, PrimitiveTopology};
use vulkano::pipeline::graphics::multisample::MultisampleState;
use vulkano::pipeline::graphics::rasterization::RasterizationState;
use vulkano::pipeline::graphics::vertex_input::{
    VertexInputAttributeDescription, VertexInputBindingDescription, VertexInputRate,
    VertexInputState,
};
use vulkano::pipeline::graphics::viewport::{Scissor, Viewport, ViewportState};
use vulkano::pipeline::layout::{PipelineLayoutCreateInfo, PushConstantRange};
use vulkano::pipeline::{
    DynamicState, GraphicsPipeline, Pipeline, PipelineBindPoint, PipelineLayout,
    PipelineShaderStageCreateInfo,
};
use vulkano::render_pass::{Framebuffer, FramebufferCreateInfo, RenderPass, Subpass};
use vulkano::shader::{ShaderModule, ShaderModuleCreateInfo, ShaderStages};
use vulkano::sync::GpuFuture;

use crate::paint::{Mesh, TextureFilter, TextureId, Vertex};

const UI_SPV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ui.spv"));
const BLUR_SPV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/blur.spv"));

// ─── push constant structs ──────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy, BufferContents)]
struct UiPushConsts {
    screen_size: [f32; 2],
    inv_screen_size: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, BufferContents)]
struct BlurPushConsts {
    inv_resolution: [f32; 2],
    offset: f32,
    _pad: f32,
}

// ─── offscreen targets ──────────────────────────────────────────────────────

/// Owns the offscreen images the compositor needs: one `scene` image the
/// host renders content into, and two blur ping-pong images. Also exposes a
/// 1×1 black "dummy" image for passes that don't need a real backdrop, and
/// the shared sampler used by [`UiRenderer`] / [`BlurRenderer`].
pub struct OffscreenTargets {
    memory_allocator: Arc<StandardMemoryAllocator>,
    render_pass: Arc<RenderPass>,
    format: Format,
    extent: [u32; 2],
    sampler: Arc<Sampler>,

    scene_view: Arc<ImageView>,
    blur_a_view: Arc<ImageView>,
    blur_b_view: Arc<ImageView>,
    dummy_view: Arc<ImageView>,

    scene_fbo: Arc<Framebuffer>,
    blur_a_fbo: Arc<Framebuffer>,
    blur_b_fbo: Arc<Framebuffer>,
}

impl OffscreenTargets {
    pub fn new(device: Arc<Device>, format: Format, extent: [u32; 2]) -> Self {
        let memory_allocator =
            Arc::new(StandardMemoryAllocator::new_default(device.clone()));

        let render_pass = vulkano::single_pass_renderpass!(
            device.clone(),
            attachments: {
                color: {
                    format: format,
                    samples: 1,
                    load_op: Clear,
                    store_op: Store,
                },
            },
            pass: {
                color: [color],
                depth_stencil: {},
            },
        )
        .expect("failed to create offscreen render pass");

        let sampler = Sampler::new(
            device.clone(),
            SamplerCreateInfo {
                mag_filter: Filter::Linear,
                min_filter: Filter::Linear,
                mipmap_mode: SamplerMipmapMode::Nearest,
                address_mode: [SamplerAddressMode::ClampToEdge; 3],
                ..Default::default()
            },
        )
        .expect("failed to create sampler");

        let (scene_view, scene_fbo) =
            make_target(&memory_allocator, &render_pass, format, extent);
        let (blur_a_view, blur_a_fbo) =
            make_target(&memory_allocator, &render_pass, format, extent);
        let (blur_b_view, blur_b_fbo) =
            make_target(&memory_allocator, &render_pass, format, extent);
        let (dummy_view, _dummy_fbo) =
            make_target(&memory_allocator, &render_pass, format, [1, 1]);

        Self {
            memory_allocator,
            render_pass,
            format,
            extent,
            sampler,
            scene_view,
            blur_a_view,
            blur_b_view,
            dummy_view,
            scene_fbo,
            blur_a_fbo,
            blur_b_fbo,
        }
    }

    pub fn recreate(&mut self, extent: [u32; 2]) {
        if extent == self.extent || extent[0] == 0 || extent[1] == 0 {
            return;
        }
        self.extent = extent;
        let (scene_view, scene_fbo) =
            make_target(&self.memory_allocator, &self.render_pass, self.format, extent);
        let (blur_a_view, blur_a_fbo) =
            make_target(&self.memory_allocator, &self.render_pass, self.format, extent);
        let (blur_b_view, blur_b_fbo) =
            make_target(&self.memory_allocator, &self.render_pass, self.format, extent);
        self.scene_view = scene_view;
        self.scene_fbo = scene_fbo;
        self.blur_a_view = blur_a_view;
        self.blur_a_fbo = blur_a_fbo;
        self.blur_b_view = blur_b_view;
        self.blur_b_fbo = blur_b_fbo;
    }

    pub fn render_pass(&self) -> &Arc<RenderPass> {
        &self.render_pass
    }
    pub fn extent(&self) -> [u32; 2] {
        self.extent
    }
    pub fn sampler(&self) -> &Arc<Sampler> {
        &self.sampler
    }
    pub fn scene_view(&self) -> &Arc<ImageView> {
        &self.scene_view
    }
    pub fn scene_fbo(&self) -> &Arc<Framebuffer> {
        &self.scene_fbo
    }
    pub fn blur_a_view(&self) -> &Arc<ImageView> {
        &self.blur_a_view
    }
    pub fn blur_a_fbo(&self) -> &Arc<Framebuffer> {
        &self.blur_a_fbo
    }
    pub fn blur_b_view(&self) -> &Arc<ImageView> {
        &self.blur_b_view
    }
    pub fn blur_b_fbo(&self) -> &Arc<Framebuffer> {
        &self.blur_b_fbo
    }
    pub fn dummy_view(&self) -> &Arc<ImageView> {
        &self.dummy_view
    }
}

fn make_target(
    memory_allocator: &Arc<StandardMemoryAllocator>,
    render_pass: &Arc<RenderPass>,
    format: Format,
    extent: [u32; 2],
) -> (Arc<ImageView>, Arc<Framebuffer>) {
    let image = Image::new(
        memory_allocator.clone(),
        ImageCreateInfo {
            image_type: ImageType::Dim2d,
            format,
            extent: [extent[0], extent[1], 1],
            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::SAMPLED,
            ..Default::default()
        },
        AllocationCreateInfo::default(),
    )
    .expect("failed to allocate offscreen image");
    let view = ImageView::new_default(image).expect("failed to create image view");
    let fbo = Framebuffer::new(
        render_pass.clone(),
        FramebufferCreateInfo {
            attachments: vec![view.clone()],
            ..Default::default()
        },
    )
    .expect("failed to create offscreen framebuffer");
    (view, fbo)
}

// ─── blur renderer ──────────────────────────────────────────────────────────

pub struct BlurRenderer {
    pipeline: Arc<GraphicsPipeline>,
    descriptor_set_allocator: Arc<StandardDescriptorSetAllocator>,
    set_layout: Arc<DescriptorSetLayout>,
}

impl BlurRenderer {
    pub fn new(device: Arc<Device>, subpass: Subpass) -> Self {
        let module = load_shader_module(&device, BLUR_SPV);
        let vs = module
            .entry_point("vs_main")
            .expect("blur vs_main missing");
        let fs = module
            .entry_point("fs_main")
            .expect("blur fs_main missing");

        let set_layout = single_sampled_image_set_layout(&device);

        let layout = PipelineLayout::new(
            device.clone(),
            PipelineLayoutCreateInfo {
                set_layouts: vec![set_layout.clone()],
                push_constant_ranges: vec![PushConstantRange {
                    stages: ShaderStages::FRAGMENT,
                    offset: 0,
                    size: std::mem::size_of::<BlurPushConsts>() as u32,
                }],
                ..Default::default()
            },
        )
        .expect("failed to create blur pipeline layout");

        let pipeline = GraphicsPipeline::new(
            device.clone(),
            None,
            GraphicsPipelineCreateInfo {
                stages: [
                    PipelineShaderStageCreateInfo::new(vs),
                    PipelineShaderStageCreateInfo::new(fs),
                ]
                .into_iter()
                .collect(),
                vertex_input_state: Some(VertexInputState::new()),
                input_assembly_state: Some(InputAssemblyState {
                    topology: PrimitiveTopology::TriangleList,
                    ..Default::default()
                }),
                viewport_state: Some(ViewportState::default()),
                rasterization_state: Some(RasterizationState::default()),
                multisample_state: Some(MultisampleState::default()),
                color_blend_state: Some(ColorBlendState::with_attachment_states(
                    subpass.num_color_attachments(),
                    ColorBlendAttachmentState::default(),
                )),
                dynamic_state: [DynamicState::Viewport, DynamicState::Scissor]
                    .into_iter()
                    .collect(),
                subpass: Some(subpass.into()),
                ..GraphicsPipelineCreateInfo::layout(layout)
            },
        )
        .expect("failed to create blur graphics pipeline");

        let descriptor_set_allocator = Arc::new(StandardDescriptorSetAllocator::new(
            device.clone(),
            Default::default(),
        ));

        Self {
            pipeline,
            descriptor_set_allocator,
            set_layout,
        }
    }

    /// Issue one blur pass: bind `src` as input, render a fullscreen triangle
    /// into the currently active render pass writing `extent` pixels with
    /// Kawase offset `offset_px` (sample distance in source pixels).
    pub fn record_pass(
        &self,
        builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
        src: Arc<ImageView>,
        sampler: Arc<Sampler>,
        extent: [u32; 2],
        offset_px: f32,
    ) {
        let descriptor_set = DescriptorSet::new(
            self.descriptor_set_allocator.clone(),
            self.set_layout.clone(),
            [
                WriteDescriptorSet::image_view(0, src),
                WriteDescriptorSet::sampler(1, sampler),
            ],
            [],
        )
        .expect("failed to allocate blur descriptor set");

        let viewport = Viewport {
            offset: [0.0, 0.0],
            extent: [extent[0] as f32, extent[1] as f32],
            depth_range: 0.0..=1.0,
        };
        let scissor = Scissor {
            offset: [0, 0],
            extent,
        };

        builder
            .bind_pipeline_graphics(self.pipeline.clone())
            .expect("bind blur pipeline failed")
            .set_viewport(0, [viewport].into_iter().collect())
            .expect("set viewport failed")
            .set_scissor(0, [scissor].into_iter().collect())
            .expect("set scissor failed")
            .bind_descriptor_sets(
                PipelineBindPoint::Graphics,
                self.pipeline.layout().clone(),
                0,
                descriptor_set,
            )
            .expect("bind blur descriptor set failed")
            .push_constants(
                self.pipeline.layout().clone(),
                0,
                BlurPushConsts {
                    inv_resolution: [1.0 / extent[0] as f32, 1.0 / extent[1] as f32],
                    offset: offset_px,
                    _pad: 0.0,
                },
            )
            .expect("blur push_constants failed");

        unsafe {
            builder
                .draw(3, 1, 0, 0)
                .expect("blur draw failed");
        }
    }
}

// ─── texture registry ───────────────────────────────────────────────────────

struct RegisteredTexture {
    view: Arc<ImageView>,
    sampler: Arc<Sampler>,
}

/// Maps opaque [`TextureId`]s to the GPU `ImageView`s widgets sample via
/// `MODE_IMAGE`. Register a vulkano `ImageView` (e.g. an engine viewport or
/// asset thumbnail) to get a `TextureId`, then pass that id to the
/// [`Image`](crate::widgets::Image) widget. The registry is handed to
/// [`UiRenderer::draw`] so it can resolve ids back to views at record time.
pub struct TextureRegistry {
    next_id: u64,
    textures: HashMap<TextureId, RegisteredTexture>,
    linear: Arc<Sampler>,
    nearest: Arc<Sampler>,
}

impl TextureRegistry {
    pub fn new(device: Arc<Device>) -> Self {
        let linear = Sampler::new(
            device.clone(),
            SamplerCreateInfo {
                mag_filter: Filter::Linear,
                min_filter: Filter::Linear,
                mipmap_mode: SamplerMipmapMode::Nearest,
                address_mode: [SamplerAddressMode::ClampToEdge; 3],
                ..Default::default()
            },
        )
        .expect("failed to create linear texture sampler");
        let nearest = Sampler::new(
            device,
            SamplerCreateInfo {
                mag_filter: Filter::Nearest,
                min_filter: Filter::Nearest,
                mipmap_mode: SamplerMipmapMode::Nearest,
                address_mode: [SamplerAddressMode::ClampToEdge; 3],
                ..Default::default()
            },
        )
        .expect("failed to create nearest texture sampler");
        Self {
            next_id: 1,
            textures: HashMap::new(),
            linear,
            nearest,
        }
    }

    /// Register `view` with the given sampling `filter` and return a fresh
    /// [`TextureId`]. The `ImageView`'s image must have `SAMPLED` usage.
    pub fn register(&mut self, view: Arc<ImageView>, filter: TextureFilter) -> TextureId {
        let sampler = match filter {
            TextureFilter::Linear => self.linear.clone(),
            TextureFilter::Nearest => self.nearest.clone(),
        };
        self.register_with_sampler(view, sampler)
    }

    /// Register `view` with a caller-provided sampler.
    pub fn register_with_sampler(
        &mut self,
        view: Arc<ImageView>,
        sampler: Arc<Sampler>,
    ) -> TextureId {
        let id = TextureId(self.next_id);
        self.next_id += 1;
        self.textures.insert(id, RegisteredTexture { view, sampler });
        id
    }

    /// Point an existing id at a new `ImageView` (e.g. the viewport image
    /// was recreated on resize). No-op if the id is unknown.
    pub fn update(&mut self, id: TextureId, view: Arc<ImageView>) {
        if let Some(tex) = self.textures.get_mut(&id) {
            tex.view = view;
        }
    }

    /// Drop a texture. Subsequent draws referencing it fall back to a dummy.
    pub fn remove(&mut self, id: TextureId) {
        self.textures.remove(&id);
    }

    fn get(&self, id: TextureId) -> Option<(&Arc<ImageView>, &Arc<Sampler>)> {
        self.textures.get(&id).map(|t| (&t.view, &t.sampler))
    }
}

// ─── ui renderer ────────────────────────────────────────────────────────────

pub struct UiRenderer {
    pipeline: Arc<GraphicsPipeline>,
    /// Ring-buffer allocator for the per-frame vertex/index streams. Arenas
    /// are recycled once the frames referencing them finish, instead of
    /// allocating fresh device memory every draw.
    stream_alloc: SubbufferAllocator,
    descriptor_set_allocator: Arc<StandardDescriptorSetAllocator>,
    set_layout: Arc<DescriptorSetLayout>,
    /// 1×1 RGBA fallback bound at the image / color-atlas slots when no real
    /// texture is in play (non-image draws never sample it).
    dummy_view: Arc<ImageView>,
    default_sampler: Arc<Sampler>,
}

impl UiRenderer {
    pub fn new(device: Arc<Device>, subpass: Subpass) -> Self {
        let memory_allocator =
            Arc::new(StandardMemoryAllocator::new_default(device.clone()));
        let descriptor_set_allocator = Arc::new(StandardDescriptorSetAllocator::new(
            device.clone(),
            Default::default(),
        ));

        let stream_alloc = SubbufferAllocator::new(
            memory_allocator.clone(),
            SubbufferAllocatorCreateInfo {
                buffer_usage: BufferUsage::VERTEX_BUFFER | BufferUsage::INDEX_BUFFER,
                memory_type_filter: MemoryTypeFilter::PREFER_HOST
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
        );

        let dummy_view = make_dummy_rgba(&memory_allocator);
        let default_sampler = Sampler::new(
            device.clone(),
            SamplerCreateInfo {
                mag_filter: Filter::Linear,
                min_filter: Filter::Linear,
                mipmap_mode: SamplerMipmapMode::Nearest,
                address_mode: [SamplerAddressMode::ClampToEdge; 3],
                ..Default::default()
            },
        )
        .expect("failed to create UI default sampler");

        let module = load_shader_module(&device, UI_SPV);
        let vs = module.entry_point("vs_main").expect("vs_main missing");
        let fs = module.entry_point("fs_main").expect("fs_main missing");

        let set_layout = ui_set_layout(&device);

        let layout = PipelineLayout::new(
            device.clone(),
            PipelineLayoutCreateInfo {
                set_layouts: vec![set_layout.clone()],
                push_constant_ranges: vec![PushConstantRange {
                    stages: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                    offset: 0,
                    size: std::mem::size_of::<UiPushConsts>() as u32,
                }],
                ..Default::default()
            },
        )
        .expect("failed to create UI pipeline layout");

        let pipeline = GraphicsPipeline::new(
            device.clone(),
            None,
            GraphicsPipelineCreateInfo {
                stages: [
                    PipelineShaderStageCreateInfo::new(vs),
                    PipelineShaderStageCreateInfo::new(fs),
                ]
                .into_iter()
                .collect(),
                vertex_input_state: Some(vertex_input_for_ui_vertex()),
                input_assembly_state: Some(InputAssemblyState {
                    topology: PrimitiveTopology::TriangleList,
                    ..Default::default()
                }),
                viewport_state: Some(ViewportState::default()),
                rasterization_state: Some(RasterizationState::default()),
                multisample_state: Some(MultisampleState::default()),
                color_blend_state: Some(ColorBlendState::with_attachment_states(
                    subpass.num_color_attachments(),
                    ColorBlendAttachmentState {
                        blend: Some(AttachmentBlend {
                            src_color_blend_factor: BlendFactor::SrcAlpha,
                            dst_color_blend_factor: BlendFactor::OneMinusSrcAlpha,
                            color_blend_op: BlendOp::Add,
                            src_alpha_blend_factor: BlendFactor::One,
                            dst_alpha_blend_factor: BlendFactor::OneMinusSrcAlpha,
                            alpha_blend_op: BlendOp::Add,
                        }),
                        ..Default::default()
                    },
                )),
                dynamic_state: [DynamicState::Viewport, DynamicState::Scissor]
                    .into_iter()
                    .collect(),
                subpass: Some(subpass.into()),
                ..GraphicsPipelineCreateInfo::layout(layout)
            },
        )
        .expect("failed to create UI graphics pipeline");

        Self {
            pipeline,
            stream_alloc,
            descriptor_set_allocator,
            set_layout,
            dummy_view,
            default_sampler,
        }
    }

    /// Upload a tessellated mesh into fresh stream buffers. Returns `None`
    /// for an empty mesh. Callers cache the result across frames (keyed by
    /// the paint-list hash) so an unchanged UI skips the re-upload; holding
    /// a [`UiBuffers`] keeps its allocator arena alive, which is safe under
    /// the renderers' existing frame fencing.
    pub fn upload(&self, mesh: &Mesh) -> Option<UiBuffers> {
        if mesh.is_empty() {
            return None;
        }
        Some(UiBuffers {
            vertices: self.upload_vertices(&mesh.vertices),
            indices: self.upload_indices(&mesh.indices),
        })
    }

    /// Record the UI mesh from previously [`upload`](Self::upload)ed buffers
    /// (`buffers` must match `mesh`). `backdrop`/`atlas` are the
    /// glassmorphism backdrop and R8 glyph atlas (constant for the mesh).
    /// `color_atlas` is the optional RGBA color-glyph page (`None` → dummy).
    /// `registry` resolves per-draw [`TextureId`]s for `MODE_IMAGE` quads.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
        mesh: &Mesh,
        buffers: &UiBuffers,
        screen_size: [f32; 2],
        backdrop: Arc<ImageView>,
        backdrop_sampler: Arc<Sampler>,
        atlas: Arc<ImageView>,
        atlas_sampler: Arc<Sampler>,
        color_atlas: Option<Arc<ImageView>>,
        color_atlas_sampler: Option<Arc<Sampler>>,
        registry: &TextureRegistry,
    ) {
        if mesh.is_empty() {
            return;
        }

        let color_atlas = color_atlas.unwrap_or_else(|| self.dummy_view.clone());
        let color_atlas_sampler =
            color_atlas_sampler.unwrap_or_else(|| self.default_sampler.clone());

        let viewport = Viewport {
            offset: [0.0, 0.0],
            extent: screen_size,
            depth_range: 0.0..=1.0,
        };

        builder
            .bind_pipeline_graphics(self.pipeline.clone())
            .expect("bind_pipeline_graphics failed")
            .set_viewport(0, [viewport].into_iter().collect())
            .expect("set_viewport failed")
            .push_constants(
                self.pipeline.layout().clone(),
                0,
                UiPushConsts {
                    screen_size,
                    inv_screen_size: [1.0 / screen_size[0], 1.0 / screen_size[1]],
                },
            )
            .expect("push_constants failed")
            .bind_vertex_buffers(0, buffers.vertices.clone())
            .expect("bind_vertex_buffers failed")
            .bind_index_buffer(buffers.indices.clone())
            .expect("bind_index_buffer failed");

        // The image slot (binding 4/5) is the only descriptor that varies
        // across draws; cache one set per distinct texture and rebind only
        // when it changes.
        let mut set_cache: HashMap<Option<TextureId>, Arc<DescriptorSet>> = HashMap::new();
        let mut bound_key: Option<Option<TextureId>> = None;

        for draw in &mesh.draws {
            let key = draw.texture;
            if bound_key != Some(key) {
                let set = set_cache
                    .entry(key)
                    .or_insert_with(|| {
                        let (image_view, image_sampler) = match key {
                            Some(id) => registry
                                .get(id)
                                .map(|(v, s)| (v.clone(), s.clone()))
                                .unwrap_or_else(|| {
                                    (self.dummy_view.clone(), self.default_sampler.clone())
                                }),
                            None => (self.dummy_view.clone(), self.default_sampler.clone()),
                        };
                        DescriptorSet::new(
                            self.descriptor_set_allocator.clone(),
                            self.set_layout.clone(),
                            [
                                WriteDescriptorSet::image_view(0, backdrop.clone()),
                                WriteDescriptorSet::sampler(1, backdrop_sampler.clone()),
                                WriteDescriptorSet::image_view(2, atlas.clone()),
                                WriteDescriptorSet::sampler(3, atlas_sampler.clone()),
                                WriteDescriptorSet::image_view(4, image_view),
                                WriteDescriptorSet::sampler(5, image_sampler),
                                WriteDescriptorSet::image_view(6, color_atlas.clone()),
                                WriteDescriptorSet::sampler(7, color_atlas_sampler.clone()),
                            ],
                            [],
                        )
                        .expect("failed to allocate UI descriptor set")
                    })
                    .clone();
                builder
                    .bind_descriptor_sets(
                        PipelineBindPoint::Graphics,
                        self.pipeline.layout().clone(),
                        0,
                        set,
                    )
                    .expect("bind_descriptor_sets failed");
                bound_key = Some(key);
            }

            let scissor = match draw.clip {
                Some(rect) => Scissor {
                    offset: [rect.min.x.max(0.0) as u32, rect.min.y.max(0.0) as u32],
                    extent: [
                        rect.width().max(0.0) as u32,
                        rect.height().max(0.0) as u32,
                    ],
                },
                None => Scissor {
                    offset: [0, 0],
                    extent: [screen_size[0] as u32, screen_size[1] as u32],
                },
            };
            builder
                .set_scissor(0, [scissor].into_iter().collect())
                .expect("set_scissor failed");
            unsafe {
                builder
                    .draw_indexed(draw.index_count, 1, draw.index_start, 0, 0)
                    .expect("draw_indexed failed");
            }
        }
    }

    fn upload_vertices(&self, vertices: &[Vertex]) -> Subbuffer<[Vertex]> {
        let buf = self
            .stream_alloc
            .allocate_slice(vertices.len() as u64)
            .expect("failed to allocate UI vertex buffer");
        buf.write()
            .expect("UI vertex buffer still in use")
            .copy_from_slice(vertices);
        buf
    }

    fn upload_indices(&self, indices: &[u32]) -> Subbuffer<[u32]> {
        let buf = self
            .stream_alloc
            .allocate_slice(indices.len() as u64)
            .expect("failed to allocate UI index buffer");
        buf.write()
            .expect("UI index buffer still in use")
            .copy_from_slice(indices);
        buf
    }
}

/// GPU vertex/index buffers for one uploaded UI mesh — see
/// [`UiRenderer::upload`].
pub struct UiBuffers {
    vertices: Subbuffer<[Vertex]>,
    indices: Subbuffer<[u32]>,
}

// ─── helpers ────────────────────────────────────────────────────────────────

fn load_shader_module(device: &Arc<Device>, spv_bytes: &[u8]) -> Arc<ShaderModule> {
    let words = spv_bytes_to_words(spv_bytes);
    // SAFETY: SPIR-V produced and validated by naga at build time.
    unsafe {
        ShaderModule::new(device.clone(), ShaderModuleCreateInfo::new(&words))
            .expect("failed to create shader module")
    }
}

/// Layout with one sampled image + sampler — used by the blur pipeline.
fn single_sampled_image_set_layout(device: &Arc<Device>) -> Arc<DescriptorSetLayout> {
    use std::collections::BTreeMap;
    let bindings = BTreeMap::from([
        (
            0,
            DescriptorSetLayoutBinding {
                stages: ShaderStages::FRAGMENT,
                ..DescriptorSetLayoutBinding::descriptor_type(DescriptorType::SampledImage)
            },
        ),
        (
            1,
            DescriptorSetLayoutBinding {
                stages: ShaderStages::FRAGMENT,
                ..DescriptorSetLayoutBinding::descriptor_type(DescriptorType::Sampler)
            },
        ),
    ]);
    DescriptorSetLayout::new(
        device.clone(),
        DescriptorSetLayoutCreateInfo {
            bindings,
            ..Default::default()
        },
    )
    .expect("failed to create descriptor set layout")
}

/// Layout with four sampled image + sampler pairs — backdrop, R8 glyph
/// atlas, user image (`MODE_IMAGE`), and RGBA color-glyph atlas. Used by
/// the UI pipeline.
fn ui_set_layout(device: &Arc<Device>) -> Arc<DescriptorSetLayout> {
    use std::collections::BTreeMap;
    let sampled = || DescriptorSetLayoutBinding {
        stages: ShaderStages::FRAGMENT,
        ..DescriptorSetLayoutBinding::descriptor_type(DescriptorType::SampledImage)
    };
    let sampler = || DescriptorSetLayoutBinding {
        stages: ShaderStages::FRAGMENT,
        ..DescriptorSetLayoutBinding::descriptor_type(DescriptorType::Sampler)
    };
    let bindings = BTreeMap::from([
        (0, sampled()),
        (1, sampler()),
        (2, sampled()),
        (3, sampler()),
        (4, sampled()),
        (5, sampler()),
        (6, sampled()),
        (7, sampler()),
    ]);
    DescriptorSetLayout::new(
        device.clone(),
        DescriptorSetLayoutCreateInfo {
            bindings,
            ..Default::default()
        },
    )
    .expect("failed to create UI descriptor set layout")
}

/// A 1×1 RGBA image used as a placeholder at the image / color-atlas
/// descriptor slots. Its contents are never sampled (the shader only reads
/// these slots in `MODE_IMAGE` / `MODE_COLOR_GLYPH`), so it's left
/// uninitialised — vulkano transitions it to a readable layout on bind.
fn make_dummy_rgba(memory_allocator: &Arc<StandardMemoryAllocator>) -> Arc<ImageView> {
    let image = Image::new(
        memory_allocator.clone(),
        ImageCreateInfo {
            image_type: ImageType::Dim2d,
            format: Format::R8G8B8A8_UNORM,
            extent: [1, 1, 1],
            usage: ImageUsage::SAMPLED | ImageUsage::TRANSFER_DST,
            ..Default::default()
        },
        AllocationCreateInfo::default(),
    )
    .expect("failed to allocate dummy image");
    ImageView::new_default(image).expect("failed to create dummy image view")
}

/// Upload a tightly-packed `R8G8B8A8_UNORM` pixel buffer to a fresh sampled
/// image and return its view, ready to hand to
/// [`TextureRegistry::register`]. Blocks until the copy completes. Intended
/// for one-off static textures (thumbnails, icons); engine-owned dynamic
/// images should be registered directly with [`TextureRegistry::register`].
///
/// `rgba` must be exactly `width * height * 4` bytes.
pub fn upload_rgba_texture(
    device: Arc<Device>,
    queue: Arc<Queue>,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Arc<ImageView> {
    assert_eq!(
        rgba.len() as u64,
        width as u64 * height as u64 * 4,
        "rgba length must equal width * height * 4"
    );

    let memory_allocator = Arc::new(StandardMemoryAllocator::new_default(device.clone()));
    let command_buffer_allocator = Arc::new(StandardCommandBufferAllocator::new(
        device.clone(),
        Default::default(),
    ));

    let staging = Buffer::from_iter(
        memory_allocator.clone(),
        BufferCreateInfo {
            usage: BufferUsage::TRANSFER_SRC,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_HOST
                | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
            ..Default::default()
        },
        rgba.iter().copied(),
    )
    .expect("failed to allocate texture staging buffer");

    let image = Image::new(
        memory_allocator,
        ImageCreateInfo {
            image_type: ImageType::Dim2d,
            format: Format::R8G8B8A8_UNORM,
            extent: [width, height, 1],
            usage: ImageUsage::SAMPLED | ImageUsage::TRANSFER_DST,
            ..Default::default()
        },
        AllocationCreateInfo::default(),
    )
    .expect("failed to allocate texture image");

    let mut builder = AutoCommandBufferBuilder::primary(
        command_buffer_allocator,
        queue.queue_family_index(),
        CommandBufferUsage::OneTimeSubmit,
    )
    .expect("failed to allocate upload command buffer");
    builder
        .copy_buffer_to_image(CopyBufferToImageInfo::buffer_image(
            staging,
            image.clone(),
        ))
        .expect("failed to record texture upload");
    let command_buffer = builder.build().expect("failed to build upload command buffer");

    vulkano::sync::now(device)
        .then_execute(queue, command_buffer)
        .expect("failed to submit texture upload")
        .then_signal_fence_and_flush()
        .expect("failed to flush texture upload")
        .wait(None)
        .expect("texture upload fence wait failed");

    ImageView::new_default(image).expect("failed to create texture image view")
}

fn spv_bytes_to_words(bytes: &[u8]) -> Vec<u32> {
    assert!(
        bytes.len().is_multiple_of(4),
        "SPIR-V byte length must be a multiple of 4"
    );
    let mut words = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        words.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    words
}

fn vertex_input_for_ui_vertex() -> VertexInputState {
    let stride = std::mem::size_of::<Vertex>() as u32;
    let mut state = VertexInputState::new();
    state = state.binding(
        0,
        VertexInputBindingDescription {
            stride,
            input_rate: VertexInputRate::Vertex,
            ..Default::default()
        },
    );

    let attrs = [
        (0u32, Format::R32G32_SFLOAT, std::mem::offset_of!(Vertex, pos)),
        (1, Format::R32G32_SFLOAT, std::mem::offset_of!(Vertex, uv)),
        (2, Format::R32G32B32A32_SFLOAT, std::mem::offset_of!(Vertex, color)),
        (3, Format::R32G32_SFLOAT, std::mem::offset_of!(Vertex, rect_center)),
        (4, Format::R32G32_SFLOAT, std::mem::offset_of!(Vertex, rect_half)),
        (5, Format::R32G32B32A32_SFLOAT, std::mem::offset_of!(Vertex, rounding)),
        (6, Format::R32G32B32A32_SFLOAT, std::mem::offset_of!(Vertex, params)),
        (7, Format::R32G32B32A32_SFLOAT, std::mem::offset_of!(Vertex, stroke_color)),
    ];
    for (loc, format, offset) in attrs {
        state = state.attribute(
            loc,
            VertexInputAttributeDescription {
                binding: 0,
                format,
                offset: offset as u32,
                ..Default::default()
            },
        );
    }
    state
}
