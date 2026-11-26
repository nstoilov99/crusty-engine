//! Shared headless GPU test harness: a windowless vulkano device driving
//! [`Context`] frames and, for snapshot tests, [`TargetRenderer`] renders
//! with golden-PNG comparison. Used by `tests/snapshots.rs` and
//! `tests/layout.rs`.
//!
//! Determinism: text is shaped with the bundled DejaVu Sans (never system
//! fonts) and frame time is injected, so output is deterministic per
//! machine. A small per-channel tolerance absorbs driver-level raster
//! differences.

#![allow(dead_code)] // not every test binary uses every helper

use std::env;
use std::fs;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crusty_gui::backend::TargetRenderer;
use crusty_gui::context::{Context, Ui};
use crusty_gui::input::{Event, Modifiers, MouseButton, RawInput};
use crusty_gui::math::{Pos2, Rect, Vec2};
use crusty_gui::paint::PaintCmd;
use crusty_gui::text::TextRenderer;

use vulkano::VulkanLibrary;
use vulkano::buffer::{Buffer, BufferCreateInfo, BufferUsage};
use vulkano::command_buffer::allocator::{
    StandardCommandBufferAllocator, StandardCommandBufferAllocatorCreateInfo,
};
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, CommandBufferUsage, CopyImageToBufferInfo,
};
use vulkano::device::physical::PhysicalDeviceType;
use vulkano::device::{Device, DeviceCreateInfo, Queue, QueueCreateInfo, QueueFlags};
use vulkano::format::Format;
use vulkano::image::view::ImageView;
use vulkano::image::{Image, ImageCreateInfo, ImageType, ImageUsage};
use vulkano::instance::{Instance, InstanceCreateFlags, InstanceCreateInfo};
use vulkano::memory::allocator::{AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator};
use vulkano::sync::GpuFuture;

const FORMAT: Format = Format::R8G8B8A8_UNORM;
pub const DT: f32 = 1.0 / 60.0;
const CLEAR: [f32; 4] = [0.09, 0.10, 0.12, 1.0];
/// Max per-channel difference that still counts as "the same pixel".
const CHANNEL_TOL: u8 = 2;
/// Fraction of pixels allowed to exceed [`CHANNEL_TOL`] (antialiased edges
/// can shift by a pixel across driver updates).
const BAD_PIXEL_FRACTION: f64 = 0.0005;

pub struct Harness {
    device: Arc<Device>,
    queue: Arc<Queue>,
    mem_alloc: Arc<StandardMemoryAllocator>,
    cmd_alloc: Arc<StandardCommandBufferAllocator>,
    renderer: TargetRenderer,
    text: TextRenderer,
    pub ctx: Context,
    target: Arc<ImageView>,
    size: [u32; 2],
    time: f64,
}

/// Build a [`Harness`], or return from the test when this machine has no usable
/// Vulkan device (GitHub's Windows runners have no ICD). Linux CI installs Mesa's
/// lavapipe, so the GPU-backed tests still run for real there and on the build
/// server; locally every dev machine has a GPU.
#[macro_export]
macro_rules! harness_or_skip {
    ($size:expr) => {
        match $crate::common::Harness::try_new($size) {
            Some(h) => h,
            None => {
                eprintln!("skipping: no Vulkan device available on this machine");
                return;
            }
        }
    };
}

impl Harness {
    pub fn new(size: [u32; 2]) -> Self {
        Self::try_new(size).expect("no usable Vulkan device")
    }

    /// `None` when there is no Vulkan loader, no instance, or no graphics-capable
    /// physical device. Everything after device selection still panics, because
    /// those failures are bugs, not missing hardware.
    pub fn try_new(size: [u32; 2]) -> Option<Self> {
        let library = VulkanLibrary::new()
            .map_err(|e| eprintln!("no local vulkan library / loader: {e}"))
            .ok()?;
        let instance = Instance::new(
            library,
            InstanceCreateInfo {
                flags: InstanceCreateFlags::ENUMERATE_PORTABILITY,
                ..Default::default()
            },
        )
        .map_err(|e| eprintln!("failed to create Vulkan instance: {e}"))
        .ok()?;

        let (physical, queue_family_index) = instance
            .enumerate_physical_devices()
            .map_err(|e| eprintln!("could not enumerate physical devices: {e}"))
            .ok()?
            .filter_map(|p| {
                p.queue_family_properties()
                    .iter()
                    .position(|q| q.queue_flags.intersects(QueueFlags::GRAPHICS))
                    .map(|i| (p, i as u32))
            })
            .min_by_key(|(p, _)| match p.properties().device_type {
                PhysicalDeviceType::DiscreteGpu => 0,
                PhysicalDeviceType::IntegratedGpu => 1,
                PhysicalDeviceType::VirtualGpu => 2,
                PhysicalDeviceType::Cpu => 3,
                _ => 4,
            })?;

        let (device, mut queues) = Device::new(
            physical,
            DeviceCreateInfo {
                queue_create_infos: vec![QueueCreateInfo {
                    queue_family_index,
                    ..Default::default()
                }],
                ..Default::default()
            },
        )
        .expect("failed to create logical device");
        let queue = queues.next().expect("no queue available");

        let mem_alloc = Arc::new(StandardMemoryAllocator::new_default(device.clone()));
        let cmd_alloc = Arc::new(StandardCommandBufferAllocator::new(
            device.clone(),
            StandardCommandBufferAllocatorCreateInfo::default(),
        ));

        let image = Image::new(
            mem_alloc.clone(),
            ImageCreateInfo {
                image_type: ImageType::Dim2d,
                format: FORMAT,
                extent: [size[0], size[1], 1],
                usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
                ..Default::default()
            },
            AllocationCreateInfo::default(),
        )
        .expect("failed to create target image");
        let target = ImageView::new_default(image).expect("failed to create target view");

        let renderer =
            TargetRenderer::new(device.clone(), queue.clone(), cmd_alloc.clone(), FORMAT);

        // Bundled font instead of whatever the OS resolves — shaping must be
        // identical across machines for goldens to be shareable.
        let mut text = TextRenderer::new(device.clone(), [1024, 1024]);
        let family = text
            .load_font(include_bytes!("../fonts/DejaVuSans.ttf").to_vec())
            .expect("failed to load bundled test font");
        text.set_default_family(family);

        Some(Self {
            device,
            queue,
            mem_alloc,
            cmd_alloc,
            renderer,
            text,
            ctx: Context::new(),
            target,
            size,
            time: 0.0,
        })
    }

    pub fn screen_rect(&self) -> Rect {
        Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(self.size[0] as f32, self.size[1] as f32),
        )
    }

    /// Run one Context frame with `events` and return its paint list.
    pub fn frame(&mut self, events: Vec<Event>, build: impl FnOnce(&mut Ui)) -> Vec<PaintCmd> {
        self.time += DT as f64;
        self.text.new_frame();
        let rect = self.screen_rect();
        self.ctx.begin_frame(RawInput {
            screen_rect: rect,
            pixels_per_point: 1.0,
            time: self.time,
            predicted_dt: DT,
            modifiers: Modifiers::empty(),
            events,
        });
        self.ctx.run_root(&mut self.text, rect, build);
        self.ctx.end_frame().paint
    }

    /// Run `n` event-less frames (settles hover/open animations) and return
    /// the last frame's paint list.
    pub fn run_frames(&mut self, n: usize, mut build: impl FnMut(&mut Ui)) -> Vec<PaintCmd> {
        let mut paint = Vec::new();
        for _ in 0..n {
            paint = self.frame(Vec::new(), &mut build);
        }
        paint
    }

    /// Render `paint` and read the framebuffer back as RGBA8.
    pub fn render_pixels(&mut self, paint: &[PaintCmd]) -> Vec<u8> {
        let ui_cb = self
            .renderer
            .render(
                &mut self.text,
                self.target.clone(),
                paint,
                None,
                Some(CLEAR),
            )
            .expect("target has zero extent");

        let readback = Buffer::new_slice::<u8>(
            self.mem_alloc.clone(),
            BufferCreateInfo {
                usage: BufferUsage::TRANSFER_DST,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_HOST
                    | MemoryTypeFilter::HOST_RANDOM_ACCESS,
                ..Default::default()
            },
            (self.size[0] * self.size[1] * 4) as u64,
        )
        .expect("failed to create readback buffer");

        let mut copy = AutoCommandBufferBuilder::primary(
            self.cmd_alloc.clone(),
            self.queue.queue_family_index(),
            CommandBufferUsage::OneTimeSubmit,
        )
        .expect("failed to allocate copy command buffer");
        copy.copy_image_to_buffer(CopyImageToBufferInfo::image_buffer(
            self.target.image().clone(),
            readback.clone(),
        ))
        .expect("failed to record image copy");
        let copy_cb = copy.build().expect("failed to build copy command buffer");

        vulkano::sync::now(self.device.clone())
            .then_execute(self.queue.clone(), ui_cb)
            .expect("failed to execute UI command buffer")
            .then_execute(self.queue.clone(), copy_cb)
            .expect("failed to execute copy command buffer")
            .then_signal_fence_and_flush()
            .expect("failed to flush")
            .wait(None)
            .expect("GPU wait failed");

        readback
            .read()
            .expect("failed to map readback buffer")
            .to_vec()
    }

    /// Render `paint`, read the pixels back and compare against the golden.
    pub fn snapshot(&mut self, name: &str, paint: &[PaintCmd]) {
        let actual = self.render_pixels(paint);
        compare(name, &actual, self.size);
    }

    /// RGBA of one pixel from a rendered frame buffer.
    pub fn pixel(&self, buf: &[u8], x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.size[0] + x) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }
}

// ── Golden compare ─────────────────────────────────────────────────────────

fn snapshots_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
}

fn compare(name: &str, actual: &[u8], size: [u32; 2]) {
    let dir = snapshots_dir();
    fs::create_dir_all(&dir).expect("failed to create snapshots dir");
    let golden_path = dir.join(format!("{name}.png"));

    if env::var_os("UPDATE_SNAPSHOTS").is_some() {
        write_png(&golden_path, actual, size);
        println!("updated golden: {}", golden_path.display());
        return;
    }

    let Some((golden, golden_size)) = read_png(&golden_path) else {
        let actual_path = dir.join(format!("{name}.actual.png"));
        write_png(&actual_path, actual, size);
        panic!(
            "missing golden {}; wrote {} — inspect it, then run with UPDATE_SNAPSHOTS=1",
            golden_path.display(),
            actual_path.display()
        );
    };
    assert_eq!(
        golden_size, size,
        "golden {name} has a different size; regenerate with UPDATE_SNAPSHOTS=1"
    );

    let total = (size[0] * size[1]) as usize;
    let mut bad = 0usize;
    let mut max_delta = 0u8;
    let mut diff = vec![0u8; total * 4];
    for (i, (a, g)) in actual
        .chunks_exact(4)
        .zip(golden.chunks_exact(4))
        .enumerate()
    {
        let d = (0..4).map(|c| a[c].abs_diff(g[c])).max().unwrap();
        max_delta = max_delta.max(d);
        if d > CHANNEL_TOL {
            bad += 1;
            diff[i * 4..i * 4 + 4].copy_from_slice(&[255, 0, 0, 255]);
        } else {
            // Dimmed golden as context around the highlighted diff pixels.
            let dim = |v: u8| v / 4;
            diff[i * 4..i * 4 + 4].copy_from_slice(&[dim(g[0]), dim(g[1]), dim(g[2]), 255]);
        }
    }

    let allowed = (total as f64 * BAD_PIXEL_FRACTION) as usize;
    if bad > allowed {
        let actual_path = dir.join(format!("{name}.actual.png"));
        let diff_path = dir.join(format!("{name}.diff.png"));
        write_png(&actual_path, actual, size);
        write_png(&diff_path, &diff, size);
        panic!(
            "snapshot {name} mismatch: {bad}/{total} pixels differ by more than \
             {CHANNEL_TOL} (allowed {allowed}, max channel delta {max_delta}); \
             wrote {} and {}",
            actual_path.display(),
            diff_path.display()
        );
    }
}

fn write_png(path: &Path, rgba: &[u8], size: [u32; 2]) {
    let file = File::create(path).expect("failed to create png");
    let mut enc = png::Encoder::new(BufWriter::new(file), size[0], size[1]);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .expect("failed to write png header")
        .write_image_data(rgba)
        .expect("failed to write png data");
}

fn read_png(path: &Path) -> Option<(Vec<u8>, [u32; 2])> {
    let file = File::open(path).ok()?;
    let mut reader = png::Decoder::new(file)
        .read_info()
        .expect("invalid golden png");
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .expect("failed to decode golden png");
    assert_eq!(info.color_type, png::ColorType::Rgba, "golden must be RGBA");
    buf.truncate(info.buffer_size());
    Some((buf, [info.width, info.height]))
}

pub fn click(pos: Pos2) -> Vec<Event> {
    vec![Event::PointerButton {
        pos,
        button: MouseButton::Primary,
        pressed: true,
        modifiers: Modifiers::empty(),
    }]
}

pub fn release(pos: Pos2) -> Vec<Event> {
    vec![Event::PointerButton {
        pos,
        button: MouseButton::Primary,
        pressed: false,
        modifiers: Modifiers::empty(),
    }]
}
