//! Phase 3 demo binary for crusty-gui.
//!
//! Adds interactive widgets driven by the immediate-mode `Ui` API:
//! pointer events from winit flow through `Context::begin_frame`, the
//! demo closure builds a `Label` and two `Button`s inside the glass
//! panel, and the resulting `PaintCmd` list is tessellated and rendered
//! through the same multi-pass pipeline from Phase 1.5.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crusty_gui::backend::vulkano::upload_rgba_texture;
use crusty_gui::prelude::*;
use crusty_gui::shell::input::apply_input_event;
use crusty_gui::shell::{DockApp, PanelFrame, Shell, ShellConfig, ShellCtl};
use crusty_gui::widgets::Window as GuiWindow;
use crusty_gui::widgets::{Canvas, CanvasView, ColorPicker, DragValue, Splitter};
use crusty_gui::window::{GpuContext, WindowRenderer};

use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{WindowAttributes, WindowId};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Quality {
    Low,
    #[default]
    Medium,
    High,
}

// Static id strings for demo selection. We use &'static str so the
// selection state can be `Copy`.
const NPC_IDS: [&str; 24] = [
    "npc_00", "npc_01", "npc_02", "npc_03", "npc_04", "npc_05",
    "npc_06", "npc_07", "npc_08", "npc_09", "npc_10", "npc_11",
    "npc_12", "npc_13", "npc_14", "npc_15", "npc_16", "npc_17",
    "npc_18", "npc_19", "npc_20", "npc_21", "npc_22", "npc_23",
];
const LIGHT_IDS: [&str; 8] = [
    "light_0", "light_1", "light_2", "light_3",
    "light_4", "light_5", "light_6", "light_7",
];
const PROP_IDS: [&str; 12] = [
    "prop_00", "prop_01", "prop_02", "prop_03", "prop_04", "prop_05",
    "prop_06", "prop_07", "prop_08", "prop_09", "prop_10", "prop_11",
];

/// A node in the demo Hierarchy panel's mutable scene tree. Real nesting so
/// drag-and-drop reparenting (`TreeNode::show_dnd`) can actually be exercised.
struct HierNode {
    id: &'static str,
    label: String,
    children: Vec<HierNode>,
}

impl HierNode {
    fn leaf(id: &'static str, label: impl Into<String>) -> Self {
        Self { id, label: label.into(), children: Vec::new() }
    }

    fn branch(id: &'static str, label: impl Into<String>, children: Vec<HierNode>) -> Self {
        Self { id, label: label.into(), children }
    }
}

fn default_hierarchy() -> Vec<HierNode> {
    vec![
        HierNode::branch("camera", "Camera", vec![
            HierNode::leaf("camera_transform", "Transform"),
            HierNode::leaf("camera_component", "CameraComponent"),
            HierNode::leaf("camera_audio", "AudioListener"),
        ]),
        HierNode::branch("sun", "Sun", vec![
            HierNode::leaf("sun_transform", "Transform"),
            HierNode::leaf("sun_light", "DirectionalLight"),
        ]),
        HierNode::branch("world", "World", vec![
            HierNode::leaf("world_terrain", "Terrain"),
            HierNode::leaf("world_foliage", "Foliage"),
            HierNode::leaf("world_water", "Water"),
            HierNode::leaf("world_skybox", "Skybox"),
        ]),
        HierNode::branch(
            "props",
            "Props",
            PROP_IDS
                .iter()
                .enumerate()
                .map(|(i, id)| HierNode::leaf(id, format!("Prop_{i:02}")))
                .collect(),
        ),
        HierNode::branch(
            "npcs",
            "NPCs",
            NPC_IDS
                .iter()
                .enumerate()
                .map(|(i, id)| HierNode::leaf(id, format!("NPC_{i:02}")))
                .collect(),
        ),
        HierNode::branch(
            "lights",
            "Lights",
            LIGHT_IDS
                .iter()
                .enumerate()
                .map(|(i, id)| HierNode::leaf(id, format!("PointLight_{i:02}")))
                .collect(),
        ),
    ]
}

/// Detach the node with `id` from wherever it sits in the tree.
fn hier_remove(nodes: &mut Vec<HierNode>, id: &str) -> Option<HierNode> {
    if let Some(i) = nodes.iter().position(|n| n.id == id) {
        return Some(nodes.remove(i));
    }
    for n in nodes.iter_mut() {
        if let Some(found) = hier_remove(&mut n.children, id) {
            return Some(found);
        }
    }
    None
}

/// Insert `node` relative to the node with `target` id. Returns the node
/// back if the target no longer exists.
fn hier_insert(
    nodes: &mut Vec<HierNode>,
    target: &str,
    pos: TreeDropPos,
    node: HierNode,
) -> Result<(), HierNode> {
    if let Some(i) = nodes.iter().position(|n| n.id == target) {
        match pos {
            TreeDropPos::Before => nodes.insert(i, node),
            TreeDropPos::After => nodes.insert(i + 1, node),
            TreeDropPos::Into => nodes[i].children.push(node),
        }
        return Ok(());
    }
    let mut node = node;
    for n in nodes.iter_mut() {
        match hier_insert(&mut n.children, target, pos, node) {
            Ok(()) => return Ok(()),
            Err(back) => node = back,
        }
    }
    Err(node)
}

fn hier_find<'t>(nodes: &'t [HierNode], id: &str) -> Option<&'t HierNode> {
    for n in nodes {
        if n.id == id {
            return Some(n);
        }
        if let Some(found) = hier_find(&n.children, id) {
            return Some(found);
        }
    }
    None
}

fn hier_subtree_contains(node: &HierNode, id: &str) -> bool {
    node.id == id || node.children.iter().any(|c| hier_subtree_contains(c, id))
}

/// Recursively render the hierarchy. Drops are collected instead of applied
/// so the tree isn't mutated while it's being iterated.
fn render_hier_nodes(
    ui: &mut Ui,
    nodes: &[HierNode],
    sel: &mut &'static str,
    drops: &mut Vec<(&'static str, &'static str, TreeDropPos)>,
) {
    for node in nodes {
        let label = if node.children.is_empty() {
            node.label.clone()
        } else {
            format!("{} ({})", node.label, node.children.len())
        };
        let out = TreeNode::new(sel, node.id, label)
            .id_source(node.id)
            .draggable(true)
            .leaf(node.children.is_empty())
            .show_dnd(ui, |ui, sel| {
                render_hier_nodes(ui, &node.children, sel, drops);
            });
        if out.response.double_clicked(ui) {
            ui.toast(format!("Double-clicked {}", node.id));
        }
        if node.id == "camera" {
            ui.context_menu_for("ctx_camera", out.response.rect, |ui| {
                let mut _unused = 0u32;
                ui.selectable_value(&mut _unused, 1, "Rename");
                ui.selectable_value(&mut _unused, 2, "Duplicate");
                ui.selectable_value(&mut _unused, 3, "Delete");
            });
        }
        if let Some((dragged, pos)) = out.drop {
            drops.push((dragged, node.id, pos));
        }
    }
}

/// Every panel `render_panel` knows how to draw. The Window menu lists these
/// so a panel that was closed (or torn off) can always be re-opened.
const ALL_PANELS: [&str; 11] = [
    "Hierarchy", "Viewport", "Inspector", "Console", "Profiler", "Errors", "Warnings",
    "Texture", "Mesh", "Animation", "Node Canvas",
];

/// Asset-editor panels — these open as their own standalone dock windows from
/// the Assets menu, the way Unreal opens a texture/mesh/anim editor. They can
/// then be docked into the main window or any other window like any tab.
const ASSET_EDITORS: [&str; 3] = ["Texture", "Mesh", "Animation"];

/// Height of the main window's top menu bar strip.
const MENU_BAR_H: f32 = 32.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Theme {
    Steel,
}

impl Theme {
    fn style(self) -> Style {
        match self {
            Theme::Steel => Style::steel(),
        }
    }
}

/// All the demo's panel-level state, bundled so the same panel content can be
/// rendered from two places: the main window's dock and a popped-out OS
/// window. `render_panel` reads/writes this; the request flags at the bottom
/// are set during a frame and consumed by the caller afterward.
struct EditorState {
    click_count: u32,
    volume: f32,
    brightness: f32,
    preview_enabled: bool,
    auto_save: bool,
    quality: Quality,
    theme: Theme,
    entity_name: String,
    asset_path: String,
    selected_entity: &'static str,
    /// Mutable scene tree shown in the Hierarchy panel — drag rows to
    /// reorder and reparent (exercises `TreeNode::show_dnd`).
    hierarchy: Vec<HierNode>,
    /// Console panel: log lines + command input with history
    /// (exercises `ScrollArea::stick_to_bottom` + `TextEdit::show_full`).
    console_lines: Vec<String>,
    console_input: String,
    console_history: Vec<String>,
    /// Index into `console_history` while navigating with Up/Down;
    /// `None` = editing a fresh line.
    console_history_pos: Option<usize>,
    inspector_slot: Option<String>,
    /// Node Canvas panel: pan/zoom view + draggable demo nodes
    /// (exercises `widgets::Canvas`, Phase 22).
    canvas_view: CanvasView,
    canvas_nodes: Vec<(Pos2, &'static str)>,
    /// In-flight node drag: (node index, world-space grab offset).
    canvas_drag: Option<(usize, Vec2)>,
    // Request flags: a panel sets these, the frame loop consumes them.
    settings_open: bool,
    about_open: bool,
    open_mesh_requested: bool,
    /// Timing figures as currently displayed (frame ms, ui µs, tess µs) —
    /// refreshed at 4 Hz so the labels don't change paint every frame.
    stats_shown: (f32, f32, f32),
    stats_shown_at: f64,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            click_count: 0,
            volume: 0.6,
            brightness: 0.8,
            preview_enabled: true,
            auto_save: false,
            quality: Quality::Medium,
            theme: Theme::Steel,
            entity_name: "PlayerCamera".to_string(),
            asset_path: "assets/scenes/demo.scene".to_string(),
            selected_entity: "",
            hierarchy: default_hierarchy(),
            console_lines: [
                "[10:23:45] Engine v0.1.0 starting up",
                "[10:23:46] Loading project 'demo-scene'...",
                "[10:23:46] Compiled 12 shaders",
                "[10:23:46] Loaded 218 entities",
                "[10:23:47] Texture streaming online (256 MB cache)",
                "[10:23:47] Audio engine started",
                "[10:23:47] Physics world initialised",
                "[10:23:47] Render thread online",
                "[10:23:48] Network: bound to 0.0.0.0:7777",
                "[10:23:48] Save subsystem ready",
                "[10:23:48] Hot-reload watcher: 142 files",
                "[10:23:48] Profiler attached on port 8080",
                "[10:23:48] Ready. Type 'help' below.",
            ]
            .map(String::from)
            .to_vec(),
            console_input: String::new(),
            console_history: Vec::new(),
            console_history_pos: None,
            inspector_slot: None,
            canvas_view: CanvasView::default(),
            canvas_nodes: vec![
                (Pos2::new(60.0, 80.0), "Event BeginPlay"),
                (Pos2::new(300.0, 40.0), "Spawn Emitter"),
                (Pos2::new(300.0, 170.0), "Play Sound"),
                (Pos2::new(540.0, 100.0), "Set Timer"),
            ],
            canvas_drag: None,
            settings_open: true,
            about_open: false,
            open_mesh_requested: false,
            stats_shown: (0.0, 0.0, 0.0),
            stats_shown_at: -1.0,
        }
    }
}

/// Per-frame, read-only figures a panel may display. Not persisted.
struct PanelStats {
    frame_time_ms: f32,
    ui_us: f32,
    tess_us: f32,
    gpu_name: String,
    demo_texture: TextureId,
    mesh_already_open: bool,
}

/// The demo's [`DockApp`]: owns all panel-level state plus the per-window
/// demo textures, and renders the menu bar, panels, and overlay windows.
/// Also owns a secondary "Mesh Editor" OS window (outside the shell's dock
/// system) driven via the `app_window_event`/`post_frame` hooks.
struct Editor {
    state: EditorState,
    window_slider: f32,
    demo_drag: f32,
    demo_color: Color,
    demo_swatches: Vec<Color>,
    gpu_name: String,
    /// Per-window checkerboard textures (each window has its own registry).
    textures: HashMap<WindowId, TextureId>,
    mesh_window: Option<WindowRenderer>,
    mesh_ctx: Context,
    mesh_pointer_pos: Option<Pos2>,
    mesh_pending_events: Vec<Event>,
    mesh_start: Option<Instant>,
    mesh_last_frame_time: f64,
    mesh_spin: f32,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            state: EditorState::default(),
            window_slider: 0.4,
            demo_drag: 60.0,
            demo_color: Color::from_srgb_u8(228, 106, 40, 255),
            demo_swatches: Vec::new(),
            gpu_name: String::new(),
            textures: HashMap::new(),
            mesh_window: None,
            mesh_ctx: Context::default(),
            mesh_pointer_pos: None,
            mesh_pending_events: Vec::new(),
            mesh_start: None,
            mesh_last_frame_time: 0.0,
            mesh_spin: 0.0,
        }
    }
}

impl DockApp for Editor {
    fn style(&self) -> Style {
        self.state.theme.style()
    }

    fn setup_window(&mut self, gpu: &GpuContext, renderer: &mut WindowRenderer) {
        self.gpu_name = gpu.gpu_name.clone();
        let id = renderer.window().id();
        let tex = build_demo_texture(gpu, renderer);
        self.textures.insert(id, tex);
    }

    fn panel_ui(&mut self, ui: &mut Ui, tab: &str, frame: &PanelFrame) {
        let stats = PanelStats {
            frame_time_ms: frame.frame_time_ms,
            ui_us: frame.ui_us,
            tess_us: frame.tess_us,
            gpu_name: self.gpu_name.clone(),
            demo_texture: self
                .textures
                .get(&frame.window)
                .copied()
                .expect("demo texture registered"),
            mesh_already_open: self.mesh_window.is_some(),
        };
        render_panel(ui, tab, &mut self.state, &stats);
    }

    fn app_window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
        modifiers: &mut Modifiers,
    ) {
        if Some(window_id) == self.mesh_window.as_ref().map(|w| w.window().id()) {
            self.mesh_event(event, modifiers);
        }
    }

    fn post_frame(&mut self, gpu: &GpuContext, event_loop: &ActiveEventLoop) -> bool {
        // A Viewport button asked to spawn the mesh window this frame.
        if self.state.open_mesh_requested && self.mesh_window.is_none() {
            self.state.open_mesh_requested = false;
            self.spawn_mesh_window(gpu, event_loop);
            return true;
        }
        false
    }

    fn top_ui(&mut self, ui: &mut Ui, ctl: &mut ShellCtl) {
        ui.menu_bar(|ui| {
            ui.menu_button("File", |ui| {
                if ui.menu_item("Save Layout") {
                    ctl.save_layout();
                }
            });
            ui.menu_button("Window", |ui| {
                for name in ALL_PANELS {
                    let open = ctl.is_open(name);
                    let mark = if open { "\u{2713} " } else { "    " };
                    if ui.menu_item(format!("{mark}{name}")) && !open {
                        ctl.open_tab(name);
                    }
                }
            });
            ui.menu_button("Assets", |ui| {
                for name in ASSET_EDITORS {
                    if ui.menu_item(format!("Open {name} Editor")) {
                        ctl.open_floating(name);
                    }
                }
            });
        });
    }

    fn overlay_ui(&mut self, ui: &mut Ui) {
        let screen_rect = ui.ctx().screen_rect();
        let editor = &mut self.state;
        let window_slider = &mut self.window_slider;
        let demo_drag = &mut self.demo_drag;
        let demo_color = &mut self.demo_color;
        let demo_swatches = &mut self.demo_swatches;

        GuiWindow::new("Settings")
            .open(&mut editor.settings_open)
            .default_pos(Pos2::new(260.0, 140.0))
            .default_size(Vec2::new(320.0, 260.0))
            .show(ui, |ui| {
                ui.label("A floating window:");
                let st = ui.style();
                Label::new("drag the title bar, resize at the\nbottom-right grip, collapse with the\nchevron, close with the cross.")
                    .size(st.fonts.small)
                    .color(st.palette.text_secondary)
                    .show(ui);
                ui.add_space(st.spacing.item);
                ui.slider(window_slider, 0.0..=1.0);
                ui.progress_bar(*window_slider);
                ui.add_space(st.spacing.item);
                ui.horizontal(|ui| {
                    ui.label("Drag value:");
                    DragValue::new(demo_drag)
                        .range(0.0..=179.0)
                        .speed(0.25)
                        .suffix("\u{00B0}")
                        .width(64.0)
                        .show(ui);
                });
                ui.horizontal(|ui| {
                    ui.label("Color:");
                    ColorPicker::new("demo_color", demo_color)
                        .swatches(demo_swatches)
                        .show(ui);
                });
                ui.add_space(st.spacing.item);
                if ui.button("Open About (modal)").clicked() {
                    editor.about_open = true;
                }
            });

        if editor.about_open {
            let mut close_clicked = false;
            GuiWindow::new("About crusty-gui")
                .open(&mut editor.about_open)
                .modal(true)
                .resizable(false)
                .collapsible(false)
                .close_on_click_outside(true)
                .default_pos(Pos2::new(
                    screen_rect.center().x - 170.0,
                    screen_rect.center().y - 110.0,
                ))
                .default_size(Vec2::new(340.0, 200.0))
                .show(ui, |ui| {
                    ui.label("crusty-gui \u{2014} Phase 9");
                    let st = ui.style();
                    Label::new("Modal window: the backdrop is dimmed\nand everything behind is blocked.\nClick outside or press Esc to close.")
                        .size(st.fonts.small)
                        .color(st.palette.text_secondary)
                        .show(ui);
                    ui.add_space(st.spacing.item);
                    if ui.button("Close").clicked() {
                        close_clicked = true;
                    }
                });
            if close_clicked {
                editor.about_open = false;
            }
        }

        // Phase 13: render & reap queued toasts on top of all UI.
        ui.show_toasts();
    }
}


fn default_dock_tree() -> DockNode {
    // Left | (top center over bottom console). Top center = viewport |
    // inspector. Console leaf has 3 tabs to showcase tab switching.
    DockNode::split_h(
        0.18,
        DockNode::leaf("Hierarchy"),
        DockNode::split_v(
            0.72,
            DockNode::split_h(
                0.72,
                DockNode::leaf("Viewport"),
                DockNode::leaf("Inspector"),
            ),
            DockNode::tabs(vec![
                "Console".into(),
                "Profiler".into(),
                "Errors".into(),
                "Warnings".into(),
            ]),
        ),
    )
}

/// Where the demo persists its dock layout + UI memory between runs.
const LAYOUT_PATH: &str = "crusty_layout.json";


impl Editor {
    /// Open the secondary "Mesh Editor" window, sharing the device/queue from
    /// the primary [`GpuContext`] but with its own swapchain and a fresh
    /// independent [`Context`]. Proves an app can own extra OS windows
    /// alongside the shell's dock windows.
    fn spawn_mesh_window(&mut self, gpu: &GpuContext, event_loop: &ActiveEventLoop) {
        let attrs = WindowAttributes::default()
            .with_title("crusty-gui — Mesh Editor")
            .with_inner_size(winit::dpi::LogicalSize::new(560.0, 440.0));
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create mesh-editor window"),
        );
        let renderer = WindowRenderer::new(gpu, window);
        renderer.window().request_redraw();
        self.mesh_window = Some(renderer);
        self.mesh_ctx = Context::default();
        self.mesh_pointer_pos = None;
        self.mesh_pending_events.clear();
        self.mesh_start = Some(Instant::now());
        self.mesh_last_frame_time = 0.0;
    }

    /// Drive the secondary window: its own input buffer, clock, and frame.
    fn mesh_event(&mut self, event: WindowEvent, modifiers: &mut Modifiers) {
        let Some(renderer) = self.mesh_window.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => {
                // Close just this window; the app keeps running.
                self.mesh_window = None;
            }
            WindowEvent::Resized(_) => {
                renderer.request_rebuild();
                renderer.window().request_redraw();
            }
            WindowEvent::RedrawRequested => {
                renderer.text_mut().new_frame();
                let size = renderer.window().inner_size();
                let pixels_per_point = renderer.window().scale_factor() as f32;
                let time = self
                    .mesh_start
                    .map(|t| t.elapsed().as_secs_f64())
                    .unwrap_or_default();
                let dt = if self.mesh_last_frame_time > 0.0 {
                    (time - self.mesh_last_frame_time).max(0.001).min(0.1) as f32
                } else {
                    1.0 / 60.0
                };
                self.mesh_last_frame_time = time;
                self.mesh_spin += dt;

                let screen_rect = Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(size.width as f32, size.height as f32),
                );
                let input = RawInput {
                    screen_rect,
                    pixels_per_point,
                    time,
                    predicted_dt: dt,
                    modifiers: *modifiers,
                    events: std::mem::take(&mut self.mesh_pending_events),
                };
                self.mesh_ctx.begin_frame(input);
                self.mesh_ctx.style = self.state.theme.style();

                let spin = self.mesh_spin;
                self.mesh_ctx.run_root(renderer.text_mut(), screen_rect, |ui| {
                    let st = ui.style();
                    ui.add_space(st.spacing.padding);
                    ui.label("Mesh Editor");
                    Label::new(
                        "A second OS window with its own Context,\nswapchain, and glass pipeline — sharing\nthe same GPU device as the main window.",
                    )
                    .size(st.fonts.small)
                    .color(st.palette.text_secondary)
                    .show(ui);
                    ui.add_space(st.spacing.item);
                    // A spinning triangle via the Phase 14 painter proves the
                    // window animates off its own independent clock.
                    let canvas = ui.allocate(Vec2::new(220.0, 220.0));
                    let c = canvas.center();
                    let accent = st.palette.accent_active;
                    let surf = st.palette.header;
                    let mut p = ui.painter();
                    p.rect_filled(canvas, 6.0, surf);
                    let r = 78.0;
                    let tau = std::f32::consts::TAU;
                    let (a0, a1, a2) = (spin, spin + tau / 3.0, spin + 2.0 * tau / 3.0);
                    p.triangle(
                        Pos2::new(c.x + r * a0.cos(), c.y + r * a0.sin()),
                        Pos2::new(c.x + r * a1.cos(), c.y + r * a1.sin()),
                        Pos2::new(c.x + r * a2.cos(), c.y + r * a2.sin()),
                        accent,
                    );
                });
                let output = self.mesh_ctx.end_frame();
                renderer.render(&[], &output.paint);
                // The spinning triangle changes the paint every frame, so this
                // gate keeps it animating while letting a static window idle.
                if output.paint_changed || output.repaint_after.is_some() {
                    renderer.window().request_redraw();
                }
            }
            other => apply_input_event(
                other,
                &mut self.mesh_pointer_pos,
                &mut self.mesh_pending_events,
                modifiers,
            ),
        }
    }
}

/// Render one dock panel's content. Shared by the main window's dock and
/// popped-out OS windows, so a torn-off panel shows its real UI.
fn render_panel(ui: &mut Ui, panel: &str, ed: &mut EditorState, stats: &PanelStats) {
    // Refresh the displayed timing figures at most 4×/sec. Displaying the raw
    // per-frame numbers would change the paint every frame, and with damage
    // tracking that self-sustains: the jittering label alone keeps the redraw
    // chain alive forever. Quantized, the paint stabilizes and the UI idles.
    // The first frame after an idle stretch reports the clamped 100ms idle
    // gap as its dt — not a real frame time, so don't display it.
    let now = ui.ctx().time();
    if now - ed.stats_shown_at >= 0.25 && stats.frame_time_ms < 99.0 {
        ed.stats_shown_at = now;
        ed.stats_shown = (stats.frame_time_ms, stats.ui_us, stats.tess_us);
    }
    let volume = &mut ed.volume;
    let brightness = &mut ed.brightness;
    let preview_enabled = &mut ed.preview_enabled;
    let auto_save = &mut ed.auto_save;
    let quality = &mut ed.quality;
    let theme = &mut ed.theme;
    let entity_name = &mut ed.entity_name;
    let asset_path = &mut ed.asset_path;
    let inspector_slot = &mut ed.inspector_slot;
    let selected_entity = &mut ed.selected_entity;
    let hierarchy = &mut ed.hierarchy;
    let console_lines = &mut ed.console_lines;
    let console_input = &mut ed.console_input;
    let console_history = &mut ed.console_history;
    let console_history_pos = &mut ed.console_history_pos;
    let click_count = &mut ed.click_count;
    let canvas_view = &mut ed.canvas_view;
    let canvas_nodes = &mut ed.canvas_nodes;
    let canvas_drag = &mut ed.canvas_drag;
    let settings_open = &mut ed.settings_open;
    let about_open = &mut ed.about_open;
    let open_mesh = &mut ed.open_mesh_requested;
    let (frame_time_ms, ui_us, tess_us) = ed.stats_shown;
    let gpu_name = &stats.gpu_name;
    let demo_texture = stats.demo_texture;
    let mesh_already_open = stats.mesh_already_open;
    match panel {
                        "Node Canvas" => {
                            render_node_canvas(ui, canvas_view, canvas_nodes, canvas_drag);
                        }
                        "Hierarchy" => {
                            let avail = ui.available().height();
                            let mut drops = Vec::new();
                            ui.scroll_area(avail, |ui| {
                                render_hier_nodes(ui, hierarchy, selected_entity, &mut drops);
                            });
                            for (dragged, target, pos) in drops {
                                // Refuse dropping a node into its own subtree —
                                // it would orphan the whole branch.
                                let cycle = hier_find(hierarchy, dragged)
                                    .is_some_and(|n| hier_subtree_contains(n, target));
                                if cycle {
                                    ui.toast_error(format!(
                                        "Can't move {dragged} into its own subtree"
                                    ));
                                    continue;
                                }
                                if let Some(node) = hier_remove(hierarchy, dragged) {
                                    match hier_insert(hierarchy, target, pos, node) {
                                        Ok(()) => {
                                            let verb = match pos {
                                                TreeDropPos::Before => "before",
                                                TreeDropPos::After => "after",
                                                TreeDropPos::Into => "into",
                                            };
                                            ui.toast_success(format!(
                                                "Moved {dragged} {verb} {target}"
                                            ));
                                        }
                                        Err(node) => hierarchy.push(node),
                                    }
                                }
                            }
                        }
                        "Viewport" => {
                            let st = ui.style();
                            let avail = ui.available().height();
                            ui.scroll_area(avail, |ui| {
                            // Phase 13: menu bar with nested submenu. Picks
                            // raise toasts so the action is visible.
                            ui.menu_bar(|ui| {
                                ui.menu_button("File", |ui| {
                                    if ui.menu_item("New Scene") {
                                        ui.toast("Created a new scene");
                                    }
                                    if ui.menu_item("Save") {
                                        ui.toast_success("Scene saved");
                                    }
                                    ui.submenu("Export", |ui| {
                                        if ui.menu_item("glTF") {
                                            ui.toast("Exported glTF");
                                        }
                                        if ui.menu_item("FBX") {
                                            ui.toast("Exported FBX");
                                        }
                                    });
                                    if ui.menu_item("Quit") {
                                        ui.toast_error("Quit is just a demo here");
                                    }
                                });
                                ui.menu_button("Edit", |ui| {
                                    if ui.menu_item("Undo") {
                                        ui.toast("Nothing to undo");
                                    }
                                    let _ = ui.menu_item("Redo");
                                });
                            });
                            ui.add_space(st.spacing.padding);
                            // Phase 13: a draggable asset chip (drag source).
                            let chip_label = "metal.png";
                            let chip = ui.allocate(Vec2::new(120.0, 24.0));
                            let chip_id = ui.alloc_id("asset_metal");
                            let dragging = ui.dnd_drag_source(
                                chip_id,
                                chip,
                                chip_label,
                                || chip_label.to_string(),
                            );
                            let chip_fill = if dragging {
                                st.palette.active
                            } else {
                                st.palette.hover
                            };
                            ui.ctx_mut().paint.push(PaintCmd::Rrect(
                                RectShape::fill(chip, st.rounding.small, chip_fill)
                                    .with_stroke(1.0, st.palette.stroke),
                            ));
                            {
                                let pos = Pos2::new(chip.min.x + 8.0, chip.min.y + 5.0);
                                let mut tmp = Vec::new();
                                ui.text_mut().add_text(
                                    &mut tmp,
                                    chip_label,
                                    st.fonts.small,
                                    pos,
                                    st.palette.text,
                                    None,
                                );
                                ui.ctx_mut().paint.extend(tmp);
                            }
                            ui.add_space(st.spacing.padding);
                            Label::new("Viewport")
                                .color(st.palette.text_secondary)
                                .show(ui);
                            ui.add_space(st.spacing.padding);
                            Label::new("Scene render placeholder")
                                .size(st.fonts.small)
                                .color(st.palette.text_secondary)
                                .show(ui);
                            ui.add_space(st.spacing.padding);
                            // Phase 14: a custom-drawn canvas via the public
                            // Painter — polyline sparkline, filled markers, a
                            // bezier, and text-at-pos, none of which the
                            // widget set covers.
                            {
                                let canvas =
                                    ui.allocate(Vec2::new(220.0, 90.0));
                                let accent = st.palette.accent_active;
                                let dim = st.palette.text_secondary;
                                let surf = st.palette.header;
                                let mut p = ui.painter();
                                p.rect_filled(canvas, 4.0, surf);
                                // Sparkline across the canvas width.
                                let n = 48;
                                let pts: Vec<Pos2> = (0..n)
                                    .map(|i| {
                                        let t = i as f32 / (n - 1) as f32;
                                        let x = canvas.min.x + 6.0
                                            + t * (canvas.width() - 12.0);
                                        let phase = t * std::f32::consts::TAU * 2.0;
                                        let y = canvas.center().y
                                            - phase.sin() * 22.0
                                            - (t * 6.0).cos() * 6.0;
                                        Pos2::new(x, y)
                                    })
                                    .collect();
                                p.polyline(&pts, 1.5, accent);
                                if let Some(last) = pts.last() {
                                    p.circle_filled(*last, 3.5, accent);
                                }
                                // A bezier swoosh under the line.
                                p.bezier_cubic(
                                    Pos2::new(canvas.min.x + 6.0, canvas.max.y - 8.0),
                                    Pos2::new(canvas.center().x - 30.0, canvas.max.y - 30.0),
                                    Pos2::new(canvas.center().x + 30.0, canvas.max.y + 4.0),
                                    Pos2::new(canvas.max.x - 6.0, canvas.max.y - 14.0),
                                    1.0,
                                    dim,
                                );
                                // A small play-triangle gizmo, top-left.
                                let g = canvas.min;
                                p.triangle(
                                    Pos2::new(g.x + 8.0, g.y + 8.0),
                                    Pos2::new(g.x + 8.0, g.y + 22.0),
                                    Pos2::new(g.x + 20.0, g.y + 15.0),
                                    accent,
                                );
                                p.text(
                                    Pos2::new(canvas.min.x + 26.0, canvas.min.y + 8.0),
                                    "painter()",
                                    12.0,
                                    dim,
                                    None,
                                );
                            }
                            ui.add_space(st.spacing.padding);
                            // Phase 10: a registered texture sampled through
                            // the new MODE_IMAGE path.
                            Image::new(demo_texture, Vec2::new(160.0, 160.0)).show(ui);
                            ui.add_space(st.spacing.padding);
                            Label::new(format!("Frame: {:.2} ms", frame_time_ms))
                                .size(st.fonts.small)
                                .color(st.palette.text_secondary)
                                .show(ui);
                            Label::new(format!("UI pass: {:.0} µs", ui_us))
                                .size(st.fonts.small)
                                .color(st.palette.text_secondary)
                                .show(ui);
                            Label::new(format!("Tessellate: {:.0} µs", tess_us))
                                .size(st.fonts.small)
                                .color(st.palette.text_secondary)
                                .show(ui);
                            Label::new(format!("Quality: {:?}", *quality))
                                .size(st.fonts.small)
                                .color(st.palette.text_secondary)
                                .show(ui);
                            ui.add_space(st.spacing.padding);
                            if ui.button("Open Settings window").clicked() {
                                *settings_open = true;
                            }
                            if ui.button("Open About (modal)").clicked() {
                                *about_open = true;
                            }
                            // Phase 15: spawn an independent second window.
                            let mesh_label = if mesh_already_open {
                                "Mesh Editor window (open)"
                            } else {
                                "Open Mesh Editor window"
                            };
                            if ui.button(mesh_label).clicked() && !mesh_already_open {
                                *open_mesh = true;
                            }
                            });
                        }
                        "Inspector" => {
                            let st = ui.style();
                            let avail = ui.available().height();
                            ui.scroll_area(avail, |ui| {
                                Label::new("Theme")
                                    .size(st.fonts.small)
                                    .color(st.palette.text_secondary)
                                    .show(ui);
                                ui.horizontal(|ui| {
                                    ui.radio_value(theme, Theme::Steel, "Steel");
                                });
                                ui.add_space(st.spacing.padding);

                                // Phase 13: drop target for the Viewport's
                                // draggable asset chip.
                                Label::new("Material")
                                    .size(st.fonts.small)
                                    .color(st.palette.text_secondary)
                                    .show(ui);
                                let slot = ui.allocate(Vec2::new(
                                    ui.available().width(),
                                    28.0,
                                ));
                                let hovering = ui.dnd_hovering::<String>(slot);
                                if let Some(name) = ui.dnd_drop_target::<String>(slot) {
                                    *inspector_slot = Some(name);
                                }
                                let slot_fill = if hovering {
                                    let mut a = st.palette.accent_active;
                                    a.a = 0.30;
                                    a
                                } else {
                                    st.palette.header
                                };
                                let slot_stroke = if hovering {
                                    st.palette.accent_active
                                } else {
                                    st.palette.stroke
                                };
                                ui.ctx_mut().paint.push(PaintCmd::Rrect(
                                    RectShape::fill(slot, st.rounding.small, slot_fill)
                                        .with_stroke(1.0, slot_stroke),
                                ));
                                {
                                    let slot_text = inspector_slot
                                        .as_deref()
                                        .unwrap_or("Drag an asset here");
                                    let pos = Pos2::new(slot.min.x + 8.0, slot.min.y + 7.0);
                                    let mut tmp = Vec::new();
                                    ui.text_mut().add_text(
                                        &mut tmp,
                                        slot_text,
                                        st.fonts.small,
                                        pos,
                                        st.palette.text,
                                        None,
                                    );
                                    ui.ctx_mut().paint.extend(tmp);
                                }
                                ui.add_space(st.spacing.padding);

                                ui.collapsing("Entity", |ui| {
                                    ui.grid("entity_props", |g| {
                                        g.cell(|ui| {
                                            Label::new("Name")
                                                .size(st.fonts.small)
                                                .color(st.palette.text_secondary)
                                                .show(ui);
                                        });
                                        g.cell(|ui| {
                                            ui.text_edit(entity_name);
                                        });
                                        g.end_row();
                                        g.cell(|ui| {
                                            Label::new("Source path")
                                                .size(st.fonts.small)
                                                .color(st.palette.text_secondary)
                                                .show(ui);
                                        });
                                        g.cell(|ui| {
                                            ui.text_edit(asset_path);
                                        });
                                        g.end_row();
                                    });
                                });
                                ui.add_space(st.spacing.padding);

                                ui.collapsing("Audio", |ui| {
                                    Label::new(format!(
                                        "Volume        {:>3.0} %",
                                        *volume * 100.0
                                    ))
                                    .show(ui);
                                    ui.slider(volume, 0.0..=1.0);
                                });
                                ui.collapsing("Display", |ui| {
                                    Label::new(format!(
                                        "Brightness    {:>3.0} %",
                                        *brightness * 100.0
                                    ))
                                    .show(ui);
                                    ui.slider(brightness, 0.0..=1.0);
                                    ui.add_space(st.spacing.item);
                                    Label::new("Render quality")
                                        .size(st.fonts.small)
                                        .color(st.palette.text_secondary)
                                        .show(ui);
                                    let quality_label = match *quality {
                                        Quality::Low => "Low",
                                        Quality::Medium => "Medium",
                                        Quality::High => "High",
                                    };
                                    ComboBox::new("quality")
                                        .selected_text(quality_label)
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                quality,
                                                Quality::Low,
                                                "Low",
                                            );
                                            ui.selectable_value(
                                                quality,
                                                Quality::Medium,
                                                "Medium",
                                            );
                                            ui.selectable_value(
                                                quality,
                                                Quality::High,
                                                "High",
                                            );
                                        });
                                });
                                ui.collapsing("Project", |ui| {
                                    ui.checkbox(preview_enabled, "Live preview");
                                    ui.checkbox(auto_save, "Auto-save");
                                });
                                ui.add_space(st.spacing.padding);

                                Label::new("Buffer fill")
                                    .size(st.fonts.small)
                                    .color(st.palette.text_secondary)
                                    .show(ui);
                                ui.progress_bar(*volume);
                                ui.add_space(st.spacing.padding);

                                let click_resp = ui.button("Click me");
                                if click_resp.clicked() {
                                    *click_count += 1;
                                }
                                ui.tooltip_for(
                                    click_resp.rect,
                                    "Increments the click counter \u{2014} try right-clicking the Camera in Hierarchy for a context menu",
                                );
                                Label::new(format!("Buttons clicked: {}", *click_count))
                                    .size(st.fonts.small)
                                    .color(st.palette.text_secondary)
                                    .show(ui);
                            });
                        }
                        "Console" => {
                            let st = ui.style();
                            // Reserve room for the input row below the log.
                            let input_h = st.fonts.body + 10.0;
                            let log_h = (ui.available().height()
                                - input_h
                                - st.spacing.item * 2.0)
                                .max(60.0);
                            ScrollArea::new(log_h)
                                .auto_shrink(false)
                                .stick_to_bottom(true)
                                .show(ui, |ui| {
                                    for line in console_lines.iter() {
                                        let color = if line.starts_with("> ") {
                                            st.palette.text
                                        } else if line.starts_with("[error]") {
                                            Color::rgba(0.95, 0.45, 0.45, 1.0)
                                        } else {
                                            st.palette.text_secondary
                                        };
                                        Label::new(line)
                                            .size(st.fonts.small)
                                            .color(color)
                                            .show(ui);
                                    }
                                });
                            ui.add_space(st.spacing.item);

                            let out = TextEdit::new(console_input)
                                .width(ui.available().width())
                                .hint("command (Enter to run, Up/Down for history)")
                                .keep_focus_on_submit(true)
                                .show_full(ui);

                            if out.submitted && !console_input.trim().is_empty() {
                                let cmd = console_input.trim().to_string();
                                console_lines.push(format!("> {cmd}"));
                                let mut parts = cmd.splitn(2, ' ');
                                match (parts.next().unwrap_or(""), parts.next()) {
                                    ("help", _) => console_lines.push(
                                        "commands: help, clear, echo <text>, gpu"
                                            .to_string(),
                                    ),
                                    ("clear", _) => console_lines.clear(),
                                    ("echo", rest) => console_lines
                                        .push(rest.unwrap_or("").to_string()),
                                    ("gpu", _) => console_lines
                                        .push(format!("Vulkano 0.35 — {gpu_name}")),
                                    (other, _) => console_lines.push(format!(
                                        "[error] unknown command '{other}'"
                                    )),
                                }
                                console_history.push(cmd);
                                *console_history_pos = None;
                                console_input.clear();
                            }
                            if out.nav_up && !console_history.is_empty() {
                                let pos = match *console_history_pos {
                                    Some(p) => p.saturating_sub(1),
                                    None => console_history.len() - 1,
                                };
                                *console_history_pos = Some(pos);
                                *console_input = console_history[pos].clone();
                            }
                            if out.nav_down {
                                match *console_history_pos {
                                    Some(p) if p + 1 < console_history.len() => {
                                        *console_history_pos = Some(p + 1);
                                        *console_input =
                                            console_history[p + 1].clone();
                                    }
                                    Some(_) => {
                                        *console_history_pos = None;
                                        console_input.clear();
                                    }
                                    None => {}
                                }
                            }
                        }
                        "Profiler" => {
                            let st = ui.style();
                            Label::new("Frame breakdown (drag a header border to resize, click to sort)")
                                .size(st.fonts.small)
                                .color(st.palette.text_secondary)
                                .show(ui);
                            ui.add_space(st.spacing.padding);

                            // 200 synthetic samples to exercise virtualization.
                            let mut rows: Vec<(usize, &str, f32)> = (0..200)
                                .map(|i| {
                                    let systems = [
                                        "render", "physics", "audio", "scripting", "ui",
                                        "animation", "networking", "streaming",
                                    ];
                                    let name = systems[i % systems.len()];
                                    let ms = 0.05 + ((i * 37 % 100) as f32) * 0.04;
                                    (i, name, ms)
                                })
                                .collect();

                            ui.table(
                                Table::new("profiler")
                                    .row_height(20.0)
                                    .max_height(220.0)
                                    .column(Column::new("#", 48.0).sortable(true))
                                    .column(
                                        Column::new("System", 140.0)
                                            .resizable(true)
                                            .sortable(true),
                                    )
                                    .column(Column::new("ms", 80.0).sortable(true)),
                                |body| {
                                    match body.sort() {
                                        Some((0, dir)) => rows.sort_by(|a, b| {
                                            let o = a.0.cmp(&b.0);
                                            if dir == SortDir::Ascending { o } else { o.reverse() }
                                        }),
                                        Some((1, dir)) => rows.sort_by(|a, b| {
                                            let o = a.1.cmp(b.1);
                                            if dir == SortDir::Ascending { o } else { o.reverse() }
                                        }),
                                        Some((2, dir)) => rows.sort_by(|a, b| {
                                            let o = a
                                                .2
                                                .partial_cmp(&b.2)
                                                .unwrap_or(std::cmp::Ordering::Equal);
                                            if dir == SortDir::Ascending { o } else { o.reverse() }
                                        }),
                                        _ => {}
                                    }
                                    body.rows(rows.len(), |i, row| {
                                        let (idx, name, ms) = rows[i];
                                        row.cell(|ui| {
                                            ui.label(format!("{idx}"));
                                        });
                                        row.cell(|ui| {
                                            ui.label(name);
                                        });
                                        row.cell(|ui| {
                                            ui.label(format!("{ms:.2}"));
                                        });
                                    });
                                },
                            );
                        }
                        "Errors" => {
                            Splitter::horizontal("errors_split")
                                .default_ratio(0.35)
                                .min_sizes(80.0, 120.0)
                                .show(
                                    ui,
                                    |left| {
                                        left.label("Sources");
                                        left.label("compiler");
                                        left.label("linker");
                                    },
                                    |right| {
                                        right.label("No errors.");
                                    },
                                );
                        }
                        "Warnings" => {
                            let st = ui.style();
                            Label::new("\u{26A0} texture 'metal.png' exceeds 2K target")
                                .size(st.fonts.small)
                                .color(st.palette.accent_active)
                                .show(ui);
                            Label::new("\u{26A0} 3 unused materials in scene")
                                .size(st.fonts.small)
                                .color(st.palette.accent_active)
                                .show(ui);
                        }
                        "Texture" => {
                            ui.label("metal_albedo.png");
                            let st = ui.style();
                            Label::new("2048 \u{00D7} 2048 \u{00B7} BC7 \u{00B7} 5.3 MB")
                                .size(st.fonts.small)
                                .color(st.palette.text_secondary)
                                .show(ui);
                            ui.add_space(st.spacing.item);
                            Image::new(demo_texture, Vec2::new(200.0, 200.0)).show(ui);
                            ui.add_space(st.spacing.item);
                            ui.checkbox(preview_enabled, "sRGB");
                            ui.checkbox(auto_save, "Generate mipmaps");
                        }
                        "Mesh" => {
                            ui.label("character_lod0.mesh");
                            let st = ui.style();
                            Label::new("12,480 tris \u{00B7} 6,210 verts \u{00B7} 1 material")
                                .size(st.fonts.small)
                                .color(st.palette.text_secondary)
                                .show(ui);
                            ui.add_space(st.spacing.item);
                            Image::new(demo_texture, Vec2::new(220.0, 160.0)).show(ui);
                            ui.add_space(st.spacing.item);
                            ui.label("LOD bias");
                            ui.slider(brightness, 0.0..=1.0);
                            ui.checkbox(preview_enabled, "Wireframe");
                        }
                        "Animation" => {
                            ui.label("run_cycle.anim");
                            let st = ui.style();
                            Label::new("32 frames \u{00B7} 30 fps \u{00B7} 1.07 s \u{00B7} looping")
                                .size(st.fonts.small)
                                .color(st.palette.text_secondary)
                                .show(ui);
                            ui.add_space(st.spacing.item);
                            ui.label("Timeline");
                            ui.slider(volume, 0.0..=1.0);
                            ui.progress_bar(*volume);
                            ui.add_space(st.spacing.item);
                            ui.checkbox(preview_enabled, "Play");
                            ui.checkbox(auto_save, "Loop");
                        }
                        _ => {}
    }
}

/// Node Canvas demo panel: exercises the Phase 22 pan/zoom [`Canvas`] —
/// world-space grid, bezier wires, and draggable nodes with zoom-quantized
/// labels. Middle-drag pans, Ctrl+scroll zooms about the pointer.
fn render_node_canvas(
    ui: &mut Ui,
    view: &mut CanvasView,
    nodes: &mut [(Pos2, &'static str)],
    drag: &mut Option<(usize, Vec2)>,
) {
    let st = ui.style();
    ui.label("Middle-drag pan · Scroll pan · Ctrl+Scroll zoom · Drag nodes");
    let node_size = Vec2::new(150.0, 46.0);

    let _ = Canvas::new().show(ui, view, |ui, scope| {
        let vis = scope.visible_world_rect();

        // Backdrop + world grid every 50 units.
        {
            let mut p = ui.painter();
            p.rect_filled(scope.rect(), 0.0, st.palette.input);
            let step = 50.0;
            for i in (vis.min.x / step).floor() as i32..=(vis.max.x / step).ceil() as i32 {
                let wx = i as f32 * step;
                let a = scope.world_to_screen(Pos2::new(wx, vis.min.y));
                let b = scope.world_to_screen(Pos2::new(wx, vis.max.y));
                p.line_segment(a, b, 1.0, st.palette.stroke);
            }
            for i in (vis.min.y / step).floor() as i32..=(vis.max.y / step).ceil() as i32 {
                let wy = i as f32 * step;
                let a = scope.world_to_screen(Pos2::new(vis.min.x, wy));
                let b = scope.world_to_screen(Pos2::new(vis.max.x, wy));
                p.line_segment(a, b, 1.0, st.palette.stroke);
            }

            // Wires: chain the demo nodes with horizontal-tangent beziers.
            for w in 0..nodes.len().saturating_sub(1) {
                let a = scope.world_to_screen(
                    nodes[w].0 + Vec2::new(node_size.x, node_size.y * 0.5),
                );
                let b = scope
                    .world_to_screen(nodes[w + 1].0 + Vec2::new(0.0, node_size.y * 0.5));
                // Tangent length grows with vertical separation too, so
                // steep connections keep a smooth S instead of pinching.
                let dx = ((b.x - a.x).abs() * 0.5)
                    .max((b.y - a.y).abs() * 0.4)
                    .max(24.0 * scope.zoom());
                p.bezier_cubic(
                    a,
                    a + Vec2::new(dx, 0.0),
                    b - Vec2::new(dx, 0.0),
                    b,
                    2.0 * scope.zoom(),
                    st.palette.accent_active,
                );
            }
        }

        // Nodes: cull, interact (world-space), draw, start drags.
        for (i, &(pos, label)) in nodes.iter().enumerate() {
            let wrect = Rect::from_min_size(pos, node_size);
            let visible = wrect.intersect(vis);
            if visible.width() <= 0.0 || visible.height() <= 0.0 {
                continue;
            }
            let id = ui.alloc_id(("canvas_node", i));
            let resp = scope.interact(ui, id, wrect);
            if let Some(pw) = scope
                .pointer_world(ui)
                .filter(|_| resp.pressed && drag.is_none())
            {
                *drag = Some((i, pw - pos));
            }

            let srect = scope.world_rect_to_screen(wrect);
            let header_h = 16.0 * scope.zoom();
            let label_px = scope.label_size(st.fonts.body);
            let mut p = ui.painter();
            let body_color = if resp.hovered {
                st.palette.hover
            } else {
                st.palette.elevated
            };
            p.rect_filled(srect, 4.0 * scope.zoom(), body_color);
            let header = Rect::from_min_size(
                srect.min,
                Vec2::new(srect.width(), header_h),
            );
            p.rect_filled(header, 4.0 * scope.zoom(), st.palette.header);
            p.rect_stroke(
                srect,
                4.0 * scope.zoom(),
                1.0,
                if resp.pressed {
                    st.palette.accent_active
                } else {
                    st.palette.stroke_strong
                },
            );
            // Pins on the vertical midline of the body.
            let pin_r = 4.0 * scope.zoom();
            let mid_y = srect.min.y + srect.height() * 0.5;
            p.circle_filled(Pos2::new(srect.min.x, mid_y), pin_r, st.palette.accent_active);
            p.circle_filled(Pos2::new(srect.max.x, mid_y), pin_r, st.palette.accent_active);
            if let Some(px) = label_px {
                p.text(
                    srect.min + Vec2::new(6.0 * scope.zoom(), header_h + 4.0 * scope.zoom()),
                    label,
                    px,
                    st.palette.text,
                    None,
                );
            }
        }

        // Advance / finish an in-flight node drag.
        if !ui.ctx().input.pointer_down {
            *drag = None;
        } else if let (Some((i, grab)), Some(pw)) = (*drag, scope.pointer_world(ui)) {
            nodes[i].0 = pw - grab;
        }
    });

    ui.mono_label(format!("zoom {:.2}", view.zoom));
}

/// Build the checkerboard demo texture and register it on `window`'s
/// texture registry, returning its [`TextureId`].
fn build_demo_texture(gpu: &GpuContext, window: &mut WindowRenderer) -> TextureId {
    const N: u32 = 64;
    let mut pixels = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let on = ((x / 8) + (y / 8)) % 2 == 0;
            let (r, g, b) = if on { (90, 130, 200) } else { (30, 40, 60) };
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    let view = upload_rgba_texture(gpu.device.clone(), gpu.queue.clone(), N, N, &pixels);
    window.registry_mut().register(view, TextureFilter::Nearest)
}

fn main() {
    let _ = env_logger::try_init();

    let event_loop = EventLoop::new().expect("failed to create event loop");
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    let mut shell = Shell::new(
        Editor::default(),
        ShellConfig {
            title: "crusty-gui — Phase 15 (multi-window)".to_string(),
            inner_size: [1280.0, 800.0],
            top_bar_height: MENU_BAR_H,
            layout_path: Some(LAYOUT_PATH.into()),
            decorator_windows: true,
            dock: default_dock_tree(),
        },
    );
    event_loop
        .run_app(&mut shell)
        .expect("event loop exited with error");
}