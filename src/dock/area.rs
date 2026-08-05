use std::collections::HashMap;

use crate::context::{Context, CursorIcon, Direction, Ui, UiOptions};
use crate::dock::tree::{DockNode, Leaf, SplitDirection, TabId};
use crate::id::Id;
use crate::math::{Color, Pos2, Rect, Rounding, Vec2};
use crate::paint::{PaintCmd, RectShape, scale_alpha};
use crate::text::TextRenderer;

const SPLITTER_THICKNESS: f32 = 5.0;
const SPLITTER_HIT_PADDING: f32 = 2.0;
const DRAG_ACTIVATION_PX: f32 = 4.0;
/// Extra tab width reserved for the × close button when enabled.
const CLOSE_BUTTON_W: f32 = 14.0;

/// Persistent dock state — drag-in-progress info that has to survive
/// across frames. Construct once in your app, pass `&mut` to
/// [`DockArea::new`] each frame.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Default)]
pub struct DockState {
    #[cfg_attr(feature = "serde", serde(skip))]
    drag: Option<DragState>,
    /// The tab whose leaf was most recently clicked (tab strip or body).
    /// Hosts use this for "focused panel" logic — e.g. which viewport's
    /// scene is active. Cleared automatically if the tab leaves the tree.
    #[cfg_attr(feature = "serde", serde(default))]
    pub focused_tab: Option<TabId>,
}

struct DragState {
    /// The tab the user picked up.
    tab_id: TabId,
    pointer_start: Pos2,
    pointer_now: Pos2,
    /// True once the pointer has moved past the activation threshold —
    /// no preview / drop zones until then, so a pure click still works
    /// for switching active tabs.
    activated: bool,
    /// Recomputed each frame by `draw_tabs`: true while the pointer is still
    /// inside the dragged tab's own tab bar. In that state the drag reorders
    /// tabs (browser-style); once it leaves the bar the tab tears off.
    home_bar: bool,
}

#[derive(Clone)]
struct DropCandidate {
    target_anchor: TabId,
    target_rect: Rect,
    /// `None` = pointer is over the panel but not over any specific
    /// compass button or its tab bar. Compass still renders so the user
    /// can see their options; drop on release is a no-op.
    zone: Option<DropZone>,
    /// True if the active drop is the panel's tab bar (visual
    /// distinction; still adds the tab to the leaf — same as `Center`).
    via_tab_bar: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DropZone {
    Center,
    North,
    South,
    East,
    West,
}

/// An incoming tab being dragged from *outside* this dock — e.g. a panel
/// that was torn off into its own OS window and is now being dragged back
/// over the main window. The host translates the global cursor into this
/// dock's local space and feeds it here so the normal 5-way drop zones
/// light up; on `released` over a zone the tab is re-inserted into the tree.
#[derive(Clone)]
pub struct ExternalDrag {
    pub tab_id: TabId,
    /// Cursor position in this dock's local coordinate space.
    pub pointer: Pos2,
    /// Cursor offset (window-local) where the source window was grabbed, so the
    /// translucent decorator preview sits under the cursor the same way the
    /// source window's title strip did. `Vec2::ZERO` is a sensible default.
    pub grab: Vec2,
    /// True on the frame the external drag is released (dropped).
    pub released: bool,
    /// Force a re-dock on release regardless of where the pointer is (used
    /// when a floating window is *closed* and we must put its tab back rather
    /// than lose it). Normal drags require releasing on a compass target.
    pub force: bool,
    /// Draw the translucent decorator card under the cursor. `false` when a
    /// real "decorator window" (Unreal-style) is following the cursor instead,
    /// so we still light up this dock's compass but don't double-draw the card.
    pub ghost: bool,
}

/// A tab that was just dragged out of the dock (off the window edge). The
/// host should spawn an OS window hosting this panel near `pointer`.
#[derive(Clone)]
pub struct DetachedTab {
    pub tab: TabId,
    /// Last in-window cursor position before the tab left the window.
    pub pointer: Pos2,
}

/// Per-leaf tab-bar geometry reported each frame so the host can place
/// trailing widgets (e.g. a "+" new-tab button) after the last tab.
#[derive(Clone)]
pub struct TabBarSlot {
    /// First tab of the leaf, identifying which leaf this bar belongs to.
    pub anchor: TabId,
    /// X just past the last tab — where a trailing widget should start.
    pub end_x: f32,
    /// The full tab-bar rect of the leaf.
    pub bar_rect: Rect,
}

/// What [`DockArea::show`] reports back to the host after a frame.
#[derive(Default)]
pub struct DockResponse {
    /// A tab was torn off the window edge this frame (already removed from
    /// the tree). Spawn an OS window for it.
    pub detached: Option<DetachedTab>,
    /// An [`ExternalDrag`] was committed into the tree this frame. The host
    /// can close the source OS window.
    pub docked: bool,
    /// A tab's × close button was clicked. The tree is *not* mutated — the
    /// host decides (veto point) and calls [`DockNode::close_tab`] to accept.
    pub close_requested: Option<TabId>,
    /// A tab was right-clicked this frame, at this pointer position. The
    /// host can open a context menu there.
    pub tab_right_clicked: Option<(TabId, Pos2)>,
    /// One entry per leaf: where its tab strip ends, for trailing widgets.
    pub tab_bar_slots: Vec<TabBarSlot>,
}

/// Per-show options threaded through layout, so tab geometry (width math in
/// render, reorder, and drop-caret paths) stays consistent everywhere.
struct LayoutOpts<'t> {
    titles: Option<&'t HashMap<TabId, String>>,
    tab_bar_h: f32,
    min_tab_width: f32,
    close_buttons: bool,
    dirty_tabs: Option<&'t std::collections::HashSet<TabId>>,
    dirty_color: Color,
}

impl LayoutOpts<'_> {
    fn title<'a>(&'a self, tab: &'a str) -> &'a str {
        self.titles
            .and_then(|m| m.get(tab))
            .map_or(tab, String::as_str)
    }
}

/// Single source of truth for a tab's width.
fn tab_width(text: &mut TextRenderer, title: &str, font: f32, opts: &LayoutOpts) -> f32 {
    let close_w = if opts.close_buttons { CLOSE_BUTTON_W } else { 0.0 };
    (text.measure(title, font, None).x + 18.0 + close_w).max(opts.min_tab_width)
}

pub struct DockArea<'a> {
    root: &'a mut DockNode,
    state: &'a mut DockState,
    external: Option<ExternalDrag>,
    lone_tearoff: bool,
    titles: Option<&'a HashMap<TabId, String>>,
    tab_bar_height: Option<f32>,
    min_tab_width: f32,
    show_close_buttons: bool,
    dirty_tabs: Option<&'a std::collections::HashSet<TabId>>,
    dirty_color: Color,
}

impl<'a> DockArea<'a> {
    pub fn new(root: &'a mut DockNode, state: &'a mut DockState) -> Self {
        Self {
            root,
            state,
            external: None,
            lone_tearoff: false,
            titles: None,
            tab_bar_height: None,
            min_tab_width: 0.0,
            show_close_buttons: false,
            dirty_tabs: None,
            dirty_color: Color::rgb(0.9, 0.75, 0.31),
        }
    }

    /// Display titles per tab id. Tabs missing from the map fall back to
    /// their id. Lets hosts keep stable ids (`"viewport:3"`) while showing
    /// live names (`"Main Scene*"`).
    pub fn titles(mut self, titles: &'a HashMap<TabId, String>) -> Self {
        self.titles = Some(titles);
        self
    }

    /// Override the tab-bar height (default: body font size + 10).
    pub fn tab_bar_height(mut self, h: f32) -> Self {
        self.tab_bar_height = Some(h);
        self
    }

    /// Minimum width for each tab (default 0 = fit to label).
    pub fn min_tab_width(mut self, w: f32) -> Self {
        self.min_tab_width = w;
        self
    }

    /// Draw an × close button on every tab. Clicks report
    /// [`DockResponse::close_requested`]; the host commits the close.
    pub fn show_close_buttons(mut self, yes: bool) -> Self {
        self.show_close_buttons = yes;
        self
    }

    /// Tabs with unsaved changes: each shows a 5px dot (in `dirty_color`)
    /// in its close-button slot, swapping back to × while hovered.
    pub fn dirty_tabs(
        mut self,
        tabs: &'a std::collections::HashSet<TabId>,
        color: Color,
    ) -> Self {
        self.dirty_tabs = Some(tabs);
        self.dirty_color = color;
        self
    }

    /// Feed an external tab drag (from another OS window) so its drop zones
    /// render and a release can re-dock it. See [`ExternalDrag`].
    pub fn external_drag(mut self, ext: Option<ExternalDrag>) -> Self {
        self.external = ext;
        self
    }

    /// Allow the *last remaining* tab to tear off too. The main window leaves
    /// this off (its lone tab has nowhere to go), but a floating host turns it
    /// on so grabbing its single tab picks the panel up as a re-dockable ghost
    /// and empties the host (which the caller then closes).
    pub fn lone_tearoff(mut self, yes: bool) -> Self {
        self.lone_tearoff = yes;
        self
    }

    /// [`show`](DockArea::show) from inside an existing `Ui` (e.g. a host
    /// that drives its whole frame through one root closure), reusing its
    /// context and text renderer.
    pub fn show_in<F>(self, ui: &mut Ui, rect: Rect, f: F) -> DockResponse
    where
        F: FnMut(&mut Ui, &str),
    {
        let Ui { ctx, text, .. } = ui;
        self.show(ctx, text, rect, f)
    }

    pub fn show<F>(
        self,
        ctx: &mut Context,
        text: &mut TextRenderer,
        rect: Rect,
        mut f: F,
    ) -> DockResponse
    where
        F: FnMut(&mut Ui, &str),
    {
        let mut response = DockResponse::default();
        // Defensive: never lay out an empty tree (would index an empty leaf).
        // The host skips empty windows, so this only guards stray callers.
        if count_tabs(self.root) == 0 {
            return response;
        }

        let opts = LayoutOpts {
            titles: self.titles,
            tab_bar_h: self
                .tab_bar_height
                .unwrap_or(ctx.style.fonts.body + 10.0),
            min_tab_width: self.min_tab_width,
            close_buttons: self.show_close_buttons,
            dirty_tabs: self.dirty_tabs,
            dirty_color: self.dirty_color,
        };

        // A tab drag lives in `DockState`, not `Memory`, so the context
        // doesn't know the UI owns the pointer — claim it explicitly or the
        // host routes the held button to the scene under the ghost.
        if self.state.drag.is_some() || self.external.is_some() {
            ctx.claim_pointer();
        }

        // ── Pre-recursion: refresh drag state from input.
        let pointer_now = ctx.input.pointer_pos;
        if let Some(drag) = self.state.drag.as_mut() {
            if let Some(p) = pointer_now {
                drag.pointer_now = p;
                if !drag.activated {
                    let d = p - drag.pointer_start;
                    if (d.x * d.x + d.y * d.y).sqrt() > DRAG_ACTIVATION_PX {
                        drag.activated = true;
                    }
                }
            }
            // Recomputed by `draw_tabs` during recursion; assume "left the bar"
            // unless a leaf claims the pointer is still over the dragged tab bar.
            drag.home_bar = false;
        }

        // ── Recursion: lay out and render chrome + content. While a drag is
        //    active, `draw_tabs` reorders the dragged tab within its own bar
        //    (browser-style) and records `home_bar`.
        let dock_root = Id::ROOT.with("dock");
        show_node(
            self.root, rect, ctx, text, &mut f, dock_root, self.state, &opts,
            &mut response,
        );

        // ── Tear-off: once the drag leaves its home tab bar, pull the tab out
        //    and report it `detached` so the host hands it to a windowless ghost
        //    drag. Inside the bar we only reorder (above), so a sideways drag
        //    rearranges tabs and a drag away detaches — like a browser.
        if let Some(drag) = self.state.drag.as_ref() {
            if drag.activated
                && !drag.home_bar
                && (count_tabs(self.root) > 1 || self.lone_tearoff)
            {
                let tab = drag.tab_id.clone();
                let pointer = drag.pointer_now;
                if remove_tab(self.root, &tab) {
                    response.detached = Some(DetachedTab { tab, pointer });
                }
                self.state.drag = None;
            }
        }

        // ── External drag (a torn-off panel dragged back over us): light
        //    up drop zones at the translated cursor and re-dock on release.
        if let Some(ext) = &self.external {
            if let Some((anchor, leaf_rect)) = locate_leaf(self.root, rect, ext.pointer) {
                // Drop semantics, with the full compass visible:
                //   • tab bar        → dock as a tab next to the others
                //   • N/S/E/W button → split the leaf in that direction
                //   • center button  → dock as a tab into the leaf
                //   • leaf body      → float (becomes its own window)
                let tab_bar_rect = Rect::from_min_max(
                    leaf_rect.min,
                    Pos2::new(leaf_rect.max.x, leaf_rect.min.y + opts.tab_bar_h),
                );
                let over_tab_bar = tab_bar_rect.contains(ext.pointer);
                let button = compute_drop_zone_compass(leaf_rect, ext.pointer);

                // Highlight the tab bar when hovered, else the hovered compass
                // button (including center — it just floats on release).
                let (vis_zone, via_tab_bar) = if over_tab_bar {
                    (Some(DropZone::Center), true)
                } else {
                    (button, false)
                };
                let candidate = DropCandidate {
                    target_anchor: anchor,
                    target_rect: leaf_rect,
                    zone: vis_zone,
                    via_tab_bar,
                };
                // Paint order: zone preview under the ghost card, compass and
                // insertion caret above it, so drop targets stay visible.
                render_drop_preview(ctx, &candidate);
                if ext.ghost {
                    render_external_decorator(
                        ctx, text, &ext.tab_id, ext.pointer, ext.grab, &mut f, &opts,
                    );
                }
                render_compass(ctx, &candidate);
                // Over the tab bar: figure out which slot the drop lands in from
                // the cursor x and draw an insertion caret there so the position
                // is explicit before release.
                let tab_slot = if over_tab_bar {
                    find_leaf(self.root, &candidate.target_anchor).map(|leaf| {
                        tab_drop_index(
                            leaf,
                            leaf_rect,
                            ext.pointer.x,
                            text,
                            ctx.style.fonts.body,
                            &opts,
                        )
                    })
                } else {
                    None
                };
                if let Some((_, caret_x)) = tab_slot {
                    let caret = Rect::from_min_max(
                        Pos2::new(caret_x, tab_bar_rect.min.y + 2.0),
                        Pos2::new(caret_x + 2.5, tab_bar_rect.max.y - 2.0),
                    );
                    ctx.paint.push(PaintCmd::Rrect(RectShape::fill(
                        caret,
                        Rounding::same(1.0),
                        ctx.style.palette.accent_active,
                    )));
                }
                if ext.released {
                    if over_tab_bar {
                        insert_at(
                            self.root,
                            &candidate.target_anchor,
                            &ext.tab_id,
                            DropZone::Center,
                            tab_slot.map(|(idx, _)| idx),
                        );
                        response.docked = true;
                    } else if let Some(zone) = button {
                        insert_at(self.root, &candidate.target_anchor, &ext.tab_id, zone, None);
                        response.docked = true;
                    }
                }
            } else if ext.released && ext.force {
                // Forced re-dock (window closed) but not over any leaf: drop it
                // onto the empty-tree root so the tab is never lost.
                if matches!(self.root, DockNode::Leaf(l) if l.tabs.is_empty()) {
                    *self.root = DockNode::leaf(ext.tab_id.as_str());
                    response.docked = true;
                }
            }
        }

        // ── Resolve on release. A drag that leaves its bar is already torn off
        //    above; one that survives here was a reorder-within-bar (committed
        //    live each frame) or a plain click (tab-switch handled in
        //    `draw_tabs`). Either way just clear it.
        if ctx.input.pointer_released {
            self.state.drag = None;
        }

        // Focus can't outlive its tab (closed, torn off, moved to another
        // window): drop stale entries so hosts never see a ghost focus.
        if self
            .state
            .focused_tab
            .as_deref()
            .is_some_and(|ft| !self.root.contains_tab(ft))
        {
            self.state.focused_tab = None;
        }

        response
    }
}

/// Total number of tabs across every leaf in the tree. Used to decide
/// whether a tab can tear off (only when more than one remains overall).
fn count_tabs(node: &DockNode) -> usize {
    let mut tabs = Vec::new();
    node.collect_tabs(&mut tabs);
    tabs.len()
}

/// Walk the layout (mirroring `show_node`'s split math) to find the leaf
/// whose rect contains `p`, returning an anchor tab id and the leaf rect.
fn locate_leaf(node: &DockNode, rect: Rect, p: Pos2) -> Option<(TabId, Rect)> {
    if !rect.contains(p) {
        return None;
    }
    match node {
        DockNode::Leaf(leaf) => {
            let anchor = leaf.tabs.first().cloned()?;
            Some((anchor, rect))
        }
        DockNode::Split(split) => {
            let (rect_a, rect_b, _) = compute_split(rect, split.direction, split.ratio);
            locate_leaf(&split.first, rect_a, p)
                .or_else(|| locate_leaf(&split.second, rect_b, p))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn show_node<F>(
    node: &mut DockNode,
    rect: Rect,
    ctx: &mut Context,
    text: &mut TextRenderer,
    f: &mut F,
    node_id: Id,
    state: &mut DockState,
    opts: &LayoutOpts,
    response: &mut DockResponse,
) where
    F: FnMut(&mut Ui, &str),
{
    match node {
        DockNode::Split(split) => {
            let (rect_a, rect_b, splitter_rect) =
                compute_split(rect, split.direction, split.ratio);
            let splitter_id = node_id.with("splitter");
            handle_splitter_drag(
                ctx,
                splitter_id,
                splitter_rect,
                split.direction,
                &mut split.ratio,
                rect,
            );
            draw_splitter(ctx, splitter_rect, splitter_id);

            show_node(
                &mut split.first, rect_a, ctx, text, f, node_id.with(0), state, opts,
                response,
            );
            show_node(
                &mut split.second, rect_b, ctx, text, f, node_id.with(1), state, opts,
                response,
            );
        }
        DockNode::Leaf(leaf) => {
            show_leaf(leaf, rect, ctx, text, f, node_id, state, opts, response);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn show_leaf<F>(
    leaf: &mut Leaf,
    rect: Rect,
    ctx: &mut Context,
    text: &mut TextRenderer,
    f: &mut F,
    node_id: Id,
    state: &mut DockState,
    opts: &LayoutOpts,
    response: &mut DockResponse,
) where
    F: FnMut(&mut Ui, &str),
{
    let style = ctx.style;
    // A hidden strip gives its row back to the content; the corner triangle
    // (drawn over the content below) is the whole restore affordance.
    let tab_bar_h = if leaf.tabs_hidden { 0.0 } else { opts.tab_bar_h };

    // Panel background.
    let panel_shape = RectShape::fill(rect, style.rounding.panel, style.palette.panel)
        .with_stroke(1.0, style.palette.stroke);
    ctx.paint.push(PaintCmd::Rrect(panel_shape));

    if !leaf.tabs_hidden {
        // Tab bar — flat strip, darker than the body so the active tab
        // (filled with the panel color) reads as connected to its content.
        let tab_bar_rect =
            Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.min.y + tab_bar_h));
        let bar_bg = RectShape::fill(tab_bar_rect, style.rounding.small, style.palette.window);
        ctx.paint.push(PaintCmd::Rrect(bar_bg));

        // Clip the tabs to their bar so a leaf with more tabs than fit its
        // width can't bleed sideways over the adjacent panel.
        ctx.paint.push(PaintCmd::PushClip(tab_bar_rect));
        draw_tabs(
            ctx, text, tab_bar_rect, &mut leaf.tabs, &mut leaf.active, state, opts,
            response,
        );
        ctx.paint.push(PaintCmd::PopClip);
    }

    let active_tab = leaf.tabs[leaf.active].clone();

    // Any click inside the leaf (tab strip or body) focuses its active tab —
    // unless an overlay (floating window, popup) owns the pointer there.
    if ctx.input.pointer_pressed && ctx.pointer_over(None, rect) {
        state.focused_tab = Some(active_tab.clone());
    }
    let content_rect = Rect::from_min_max(
        Pos2::new(rect.min.x, rect.min.y + tab_bar_h),
        rect.max,
    );
    let leaf_root_id = node_id.with("leaf").with(&active_tab);
    let opts = UiOptions {
        padding: Vec2::splat(style.spacing.padding),
        spacing: style.spacing.item,
    };
    // Scissor the body to the panel so content that overflows can neither
    // draw over adjacent panels nor be clicked through them — the Ui's clip
    // (set to `content_rect` by `with_root`) handles hit testing; this clip
    // handles paint.
    ctx.paint.push(PaintCmd::PushClip(content_rect));
    let mut ui = Ui::with_root(ctx, text, content_rect, Direction::TopDown, opts, leaf_root_id);
    f(&mut ui, &active_tab);
    if leaf.tabs_hidden {
        show_tabs_triangle(&mut ui, node_id, rect, &mut leaf.tabs_hidden);
    }
    ctx.paint.push(PaintCmd::PopClip);
}

/// Corner triangle drawn over the content while the tab strip is hidden —
/// 12px accent at rest, 14px focus-ring + tooltip on hover; click restores
/// the strip in place (mockup "Editor tabs" § C).
fn show_tabs_triangle(ui: &mut Ui, node_id: Id, rect: Rect, hidden: &mut bool) {
    let hit = Rect::from_min_size(rect.min, Vec2::splat(16.0));
    let resp = ui.interact(node_id.with("show_tabs"), hit);
    let style = ui.style();
    let (size, color) = if resp.hovered {
        (14.0, style.palette.focus_ring)
    } else {
        (12.0, style.palette.accent_active)
    };
    ui.ctx_mut().paint.push(PaintCmd::ConvexPolygon {
        points: vec![
            rect.min,
            Pos2::new(rect.min.x + size, rect.min.y),
            Pos2::new(rect.min.x, rect.min.y + size),
        ],
        color,
    });
    if resp.hovered {
        crate::widgets::show_tooltip_for(ui, hit, "Show tabs");
    }
    if resp.clicked {
        *hidden = false;
    }
}

fn compute_split(rect: Rect, dir: SplitDirection, ratio: f32) -> (Rect, Rect, Rect) {
    let half = SPLITTER_THICKNESS * 0.5;
    match dir {
        SplitDirection::Horizontal => {
            let split_x = rect.min.x + rect.width() * ratio;
            let a = Rect::from_min_max(rect.min, Pos2::new(split_x - half, rect.max.y));
            let b = Rect::from_min_max(Pos2::new(split_x + half, rect.min.y), rect.max);
            let s = Rect::from_min_max(
                Pos2::new(split_x - half - SPLITTER_HIT_PADDING, rect.min.y),
                Pos2::new(split_x + half + SPLITTER_HIT_PADDING, rect.max.y),
            );
            (a, b, s)
        }
        SplitDirection::Vertical => {
            let split_y = rect.min.y + rect.height() * ratio;
            let a = Rect::from_min_max(rect.min, Pos2::new(rect.max.x, split_y - half));
            let b = Rect::from_min_max(Pos2::new(rect.min.x, split_y + half), rect.max);
            let s = Rect::from_min_max(
                Pos2::new(rect.min.x, split_y - half - SPLITTER_HIT_PADDING),
                Pos2::new(rect.max.x, split_y + half + SPLITTER_HIT_PADDING),
            );
            (a, b, s)
        }
    }
}

fn handle_splitter_drag(
    ctx: &mut Context,
    splitter_id: Id,
    splitter_rect: Rect,
    dir: SplitDirection,
    ratio: &mut f32,
    parent_rect: Rect,
) {
    let hovered = ctx.pointer_over(None, splitter_rect);
    let pointer_pressed = ctx.input.pointer_pressed;
    let pointer_down = ctx.input.pointer_down;
    if pointer_pressed && hovered {
        ctx.memory.active_widget = Some(splitter_id);
    }
    let is_active = ctx.memory.active_widget == Some(splitter_id);
    if hovered || is_active {
        ctx.set_cursor_icon(match dir {
            SplitDirection::Horizontal => CursorIcon::ResizeEw,
            SplitDirection::Vertical => CursorIcon::ResizeNs,
        });
    }
    if is_active && pointer_down {
        if let Some(p) = ctx.input.pointer_pos {
            *ratio = match dir {
                SplitDirection::Horizontal => {
                    ((p.x - parent_rect.min.x) / parent_rect.width()).clamp(0.05, 0.95)
                }
                SplitDirection::Vertical => {
                    ((p.y - parent_rect.min.y) / parent_rect.height()).clamp(0.05, 0.95)
                }
            };
        }
    }
}

fn draw_splitter(ctx: &mut Context, splitter_rect: Rect, splitter_id: Id) {
    let style = ctx.style;
    let hovered = ctx.pointer_over(None, splitter_rect);
    let is_active = ctx.memory.active_widget == Some(splitter_id);
    let highlight = if is_active {
        1.0
    } else if hovered {
        0.55
    } else {
        0.0
    };
    let color = lerp_color(style.palette.stroke, style.palette.accent_active, highlight);
    let shape = RectShape::fill(splitter_rect, Rounding::ZERO, color);
    ctx.paint.push(PaintCmd::Rrect(shape));
}

#[allow(clippy::too_many_arguments)]
fn draw_tabs(
    ctx: &mut Context,
    text: &mut TextRenderer,
    tab_bar_rect: Rect,
    tabs: &mut Vec<TabId>,
    active: &mut usize,
    state: &mut DockState,
    opts: &LayoutOpts,
    response: &mut DockResponse,
) {
    let style = ctx.style;

    // ── Reorder-within-bar: if an active drag owns one of these tabs and the
    //    pointer is still inside this bar, slide the dragged tab to the slot
    //    under the cursor (browser-style) and mark `home_bar` so the caller
    //    reorders instead of tearing off.
    let drag_tab = state
        .drag
        .as_ref()
        .filter(|d| d.activated)
        .map(|d| d.tab_id.clone());
    if let Some(dtab) = drag_tab {
        if let Some(from) = tabs.iter().position(|t| *t == dtab) {
            let in_bar = ctx
                .input
                .pointer_pos
                .map_or(false, |p| tab_bar_rect.contains(p));
            if let Some(drag) = state.drag.as_mut() {
                drag.home_bar = in_bar;
            }
            if let (true, Some(px)) = (in_bar, ctx.input.pointer_pos.map(|p| p.x)) {
                let mut cx = tab_bar_rect.min.x + 6.0;
                let mut to = tabs.len() - 1;
                for (i, tab) in tabs.iter().enumerate() {
                    let w = tab_width(text, opts.title(tab), style.fonts.body, opts);
                    if px < cx + w * 0.5 {
                        to = i;
                        break;
                    }
                    cx += w + 4.0;
                }
                if to != from {
                    let t = tabs.remove(from);
                    tabs.insert(to, t);
                }
                *active = to;
            }
        }
    }

    let mut x = tab_bar_rect.min.x + 6.0;
    let bar_h = tab_bar_rect.height();
    let active_was = *active;
    for (i, tab) in tabs.iter().enumerate() {
        let title = opts.title(tab);
        let label_size = text.measure(title, style.fonts.body, None);
        let tab_w = tab_width(text, title, style.fonts.body, opts);
        let tab_rect = Rect::from_min_size(
            Pos2::new(x, tab_bar_rect.min.y + 1.0),
            Vec2::new(tab_w, bar_h - 1.0),
        );

        let is_active = i == active_was;
        let hovered = ctx.pointer_over(None, tab_rect);
        // Inactive tabs sit flat on the strip (no chip fill); the active tab
        // is filled with the panel color so it merges with the body below,
        // capped by an accent line along its top edge.
        if is_active {
            ctx.paint.push(PaintCmd::Rrect(RectShape::fill(
                tab_rect,
                style.rounding.small,
                style.palette.panel,
            )));
            let top_line = Rect::from_min_max(
                tab_rect.min,
                Pos2::new(tab_rect.max.x, tab_rect.min.y + 2.0),
            );
            ctx.paint.push(PaintCmd::Rrect(RectShape::fill(
                top_line,
                Rounding::same(1.0),
                style.palette.accent_active,
            )));
        } else if hovered {
            ctx.paint.push(PaintCmd::Rrect(RectShape::fill(
                tab_rect,
                style.rounding.small,
                style.palette.hover,
            )));
        }

        let text_pos = Pos2::new(
            tab_rect.min.x + 9.0,
            tab_rect.min.y + (tab_rect.height() - label_size.y) * 0.5,
        );
        let label_color = if is_active {
            style.palette.text
        } else {
            style.palette.text_secondary
        };
        text.add_text(
            &mut ctx.paint,
            title,
            style.fonts.body,
            text_pos,
            label_color,
            None,
        );

        // × close button, right-aligned inside the tab. Clicks report a
        // close request (host commits) and never start a tab drag.
        let mut over_close = false;
        if opts.close_buttons {
            let s = 12.0;
            let close_rect = Rect::from_center_size(
                Pos2::new(tab_rect.max.x - 4.0 - s * 0.5, tab_rect.center().y),
                Vec2::splat(s),
            );
            over_close = ctx.pointer_over(None, close_rect);
            if over_close {
                ctx.paint.push(PaintCmd::Rrect(RectShape::fill(
                    close_rect,
                    style.rounding.small,
                    style.palette.hover,
                )));
            }
            let dirty = opts.dirty_tabs.is_some_and(|d| d.contains(tab));
            // A dirty tab shows a 5px warning dot in the close slot; hovering
            // the tab swaps it back to × so the close action stays reachable.
            if dirty && !hovered {
                let dot = Rect::from_center_size(close_rect.center(), Vec2::splat(5.0));
                ctx.paint.push(PaintCmd::Rrect(RectShape::fill(
                    dot,
                    Rounding::same(2.5),
                    opts.dirty_color,
                )));
            }
            let show_glyph = (is_active || hovered) && !(dirty && !hovered);
            if show_glyph {
                let g = close_rect.shrink(3.5);
                let col = if over_close {
                    style.palette.text
                } else {
                    style.palette.text_secondary
                };
                ctx.paint.push(PaintCmd::Line {
                    from: g.min,
                    to: g.max,
                    width: 1.2,
                    color: col,
                });
                ctx.paint.push(PaintCmd::Line {
                    from: Pos2::new(g.max.x, g.min.y),
                    to: Pos2::new(g.min.x, g.max.y),
                    width: 1.2,
                    color: col,
                });
            }
            if ctx.input.pointer_pressed && over_close {
                response.close_requested = Some(tab.clone());
            }
        }

        if let Some(p) = ctx
            .input
            .pointer_pos
            .filter(|_| ctx.input.right_pressed && hovered)
        {
            response.tab_right_clicked = Some((tab.clone(), p));
        }

        // Start a drag on press (don't switch yet — switching happens
        // on release without movement).
        if ctx.input.pointer_pressed && hovered && !over_close && state.drag.is_none() {
            if let Some(p) = ctx.input.pointer_pos {
                state.drag = Some(DragState {
                    tab_id: tab.clone(),
                    pointer_start: p,
                    pointer_now: p,
                    activated: false,
                    home_bar: false,
                });
            }
        }

        // Click without drag = switch active tab (and focus it).
        if ctx.input.pointer_released && hovered {
            if let Some(drag) = state.drag.as_ref() {
                if !drag.activated && drag.tab_id == *tab {
                    *active = i;
                    state.focused_tab = Some(tab.clone());
                }
            }
        }

        x += tab_w + 4.0;
    }

    // Report where the strip ends so the host can place trailing widgets
    // (e.g. a "+" new-tab button) in the leftover bar space.
    if let Some(anchor) = tabs.first() {
        response.tab_bar_slots.push(TabBarSlot {
            anchor: anchor.clone(),
            end_x: x,
            bar_rect: tab_bar_rect,
        });
    }
}

// ─── compass + preview overlay (egui_dock-style) ───────────────────────────

const COMPASS_BUTTON_SIZE: f32 = 38.0;
const COMPASS_BUTTON_GAP: f32 = 6.0;

/// Five small buttons clustered at the panel center: Center in the middle,
/// N/S/E/W as a cross around it.
fn compass_button_rects(leaf: Rect) -> [(DropZone, Rect); 5] {
    let cx = leaf.center();
    let s = COMPASS_BUTTON_SIZE;
    let off = s + COMPASS_BUTTON_GAP;
    let center = Rect::from_center_size(cx, Vec2::splat(s));
    let north = Rect::from_center_size(Pos2::new(cx.x, cx.y - off), Vec2::splat(s));
    let south = Rect::from_center_size(Pos2::new(cx.x, cx.y + off), Vec2::splat(s));
    let east = Rect::from_center_size(Pos2::new(cx.x + off, cx.y), Vec2::splat(s));
    let west = Rect::from_center_size(Pos2::new(cx.x - off, cx.y), Vec2::splat(s));
    [
        (DropZone::Center, center),
        (DropZone::North, north),
        (DropZone::South, south),
        (DropZone::East, east),
        (DropZone::West, west),
    ]
}

/// Which compass button (if any) the pointer is inside.
fn compute_drop_zone_compass(leaf: Rect, p: Pos2) -> Option<DropZone> {
    for (zone, rect) in compass_button_rects(leaf) {
        if rect.contains(p) {
            return Some(zone);
        }
    }
    None
}

/// Where the dropped tab will end up — used for the preview overlay.
fn preview_rect_for_zone(leaf: Rect, zone: DropZone) -> Rect {
    let mid_x = leaf.center().x;
    let mid_y = leaf.center().y;
    match zone {
        DropZone::Center => leaf,
        DropZone::North => Rect::from_min_max(leaf.min, Pos2::new(leaf.max.x, mid_y)),
        DropZone::South => Rect::from_min_max(Pos2::new(leaf.min.x, mid_y), leaf.max),
        DropZone::West => Rect::from_min_max(leaf.min, Pos2::new(mid_x, leaf.max.y)),
        DropZone::East => Rect::from_min_max(Pos2::new(mid_x, leaf.min.y), leaf.max),
    }
}

/// Translucent zone-preview fill; drawn below the drag decorator so the
/// ghost card isn't tinted.
fn render_drop_preview(ctx: &mut Context, candidate: &DropCandidate) {
    let style = ctx.style;
    if candidate.via_tab_bar {
        // Drop on tab bar = add as a tab here — the exact insertion slot is
        // shown by the caret drawn in the caller, so don't tint the whole bar.
    } else if let Some(zone) = candidate.zone {
        let preview = preview_rect_for_zone(candidate.target_rect, zone);
        let mut tint = style.palette.accent_active;
        tint.a = 0.22;
        let mut stroke = style.palette.accent_active;
        stroke.a = 0.95;
        // Plain quad, not an rrect fill: translucent rrect fills take the
        // shader's glass path, which composites the backdrop opaquely.
        ctx.paint.push(PaintCmd::Rect {
            rect: preview,
            color: tint,
        });
        let mut border = RectShape::fill(preview, style.rounding.widget, stroke)
            .with_stroke(2.0, stroke);
        border.fill = None;
        ctx.paint.push(PaintCmd::Rrect(border));
    }
}

/// Compass buttons; drawn above the drag decorator so the drop targets stay
/// visible under the ghost card.
fn render_compass(ctx: &mut Context, candidate: &DropCandidate) {
    let style = ctx.style;
    let buttons = compass_button_rects(candidate.target_rect);
    for (zone, btn_rect) in buttons {
        let is_active = Some(zone) == candidate.zone && !candidate.via_tab_bar;
        let mut fill = style.palette.header;
        fill.a = if is_active { 0.98 } else { 0.92 };
        let (stroke_w, stroke) = if is_active {
            (1.5, style.palette.accent_active)
        } else {
            (1.0, style.palette.stroke)
        };
        ctx.paint.push(PaintCmd::Rrect(
            RectShape::fill(btn_rect, style.rounding.small, fill)
                .with_stroke(stroke_w, stroke),
        ));
        draw_compass_icon(ctx, btn_rect, zone, is_active);
    }
}

/// egui_dock-style compass glyph: a mini window (title-bar strip + faint
/// border) with a dashed line previewing the split axis; Center has no
/// dashes (drop merges as a tab).
fn draw_compass_icon(ctx: &mut Context, btn_rect: Rect, zone: DropZone, active: bool) {
    let style = ctx.style;
    let ink = if active {
        style.palette.text
    } else {
        style.palette.text_secondary
    };
    let icon = btn_rect.shrink(7.0);

    ctx.paint.push(PaintCmd::Rrect(
        RectShape::fill(icon, Rounding::same(1.0), Color::TRANSPARENT)
            .with_stroke(1.0, ink.with_alpha(0.35)),
    ));
    let bar = Rect::from_min_size(icon.min, Vec2::new(icon.width(), 4.0));
    ctx.paint
        .push(PaintCmd::Rrect(RectShape::fill(bar, Rounding::same(1.0), ink)));

    // Dashed split preview through the window body.
    let body = Rect::from_min_max(Pos2::new(icon.min.x, bar.max.y), icon.max);
    let c = body.center();
    let inset = 2.5;
    match zone {
        DropZone::Center => {}
        DropZone::North | DropZone::South => dashed_line(
            ctx,
            Pos2::new(body.min.x + inset, c.y),
            Pos2::new(body.max.x - inset, c.y),
            ink,
        ),
        DropZone::East | DropZone::West => dashed_line(
            ctx,
            Pos2::new(c.x, body.min.y + inset),
            Pos2::new(c.x, body.max.y - inset),
            ink,
        ),
    }
}

/// Axis-aligned dashed line built from small rect segments.
fn dashed_line(ctx: &mut Context, from: Pos2, to: Pos2, color: Color) {
    const DASH: f32 = 3.0;
    const GAP: f32 = 2.5;
    const THICK: f32 = 1.5;
    let horizontal = (to.y - from.y).abs() < 0.5;
    let len = if horizontal { to.x - from.x } else { to.y - from.y };
    let mut t = 0.0;
    while t < len {
        let seg = DASH.min(len - t);
        let r = if horizontal {
            Rect::from_min_size(
                Pos2::new(from.x + t, from.y - THICK * 0.5),
                Vec2::new(seg, THICK),
            )
        } else {
            Rect::from_min_size(
                Pos2::new(from.x - THICK * 0.5, from.y + t),
                Vec2::new(THICK, seg),
            )
        };
        ctx.paint
            .push(PaintCmd::Rrect(RectShape::fill(r, Rounding::ZERO, color)));
        t += DASH + GAP;
    }
}

/// Width/height of the translucent decorator preview shown while a whole
/// window is dragged to re-dock. Mirrors Unreal's drag behaviour: the panel's
/// real content follows the cursor semi-transparently, with no OS chrome/logo.
pub const DECORATOR_SIZE: Vec2 = Vec2::new(360.0, 280.0);

/// Render a translucent "decorator" preview of the dragged window at the
/// cursor: a panel-shaped card with a slim title pill plus the panel's *actual*
/// content, the whole thing faded so the drop indicators beneath stay visible.
/// Replaces the old label chip so re-docking looks like dragging the real
/// window, exactly like Unreal Engine.
fn render_external_decorator<F>(
    ctx: &mut Context,
    text: &mut TextRenderer,
    tab_id: &str,
    pointer: Pos2,
    grab: Vec2,
    f: &mut F,
    opts: &LayoutOpts,
) where
    F: FnMut(&mut Ui, &str),
{
    let style = ctx.style;
    let tab_bar_h = opts.tab_bar_h;
    // Place the card so the cursor sits where the title strip was grabbed.
    let origin = Pos2::new(pointer.x - grab.x, pointer.y - grab.y);
    let rect = Rect::from_min_size(origin, DECORATOR_SIZE);

    // Everything from here is faded together at the end.
    let start = ctx.paint.len();

    // Panel body + slim header bar carrying the tab name (no window chrome).
    // Translucent so the closing `scale_alpha` actually fades them — glass
    // mode would read that alpha as a tint strength and stay opaque, hiding
    // the drop compass this decorator is meant to float above.
    let body = RectShape::fill_translucent(rect, style.rounding.panel, style.palette.panel)
        .with_stroke(1.0, style.palette.accent_active);
    ctx.paint.push(PaintCmd::Rrect(body));
    let header = Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.min.y + tab_bar_h));
    ctx.paint.push(PaintCmd::Rrect(RectShape::fill_translucent(
        header,
        style.rounding.small,
        style.palette.window,
    )));
    text.add_text(
        &mut ctx.paint,
        opts.title(tab_id),
        style.fonts.body,
        Pos2::new(header.min.x + 8.0, header.min.y + 5.0),
        style.palette.text,
        None,
    );

    // The panel's real content, clipped to the card body.
    let content_rect = Rect::from_min_max(
        Pos2::new(rect.min.x, rect.min.y + tab_bar_h),
        rect.max,
    );
    ctx.paint.push(PaintCmd::PushClip(content_rect));
    let opts = UiOptions {
        padding: Vec2::splat(style.spacing.padding),
        spacing: style.spacing.item,
    };
    let mut ui =
        Ui::with_root(ctx, text, content_rect, Direction::TopDown, opts, Id::ROOT.with("dock_ghost"));
    f(&mut ui, tab_id);
    ctx.paint.push(PaintCmd::PopClip);

    // Fade the whole decorator so the host's drop compass shows through.
    scale_alpha(&mut ctx.paint[start..], 0.6);
}

// ─── tree mutation ──────────────────────────────────────────────────────────

impl DockNode {
    /// Remove `tab` from the tree, collapsing any split left with an empty
    /// side. Returns true if the tab was found. Call this to commit a
    /// [`DockResponse::close_requested`] (or skip it to veto the close).
    pub fn close_tab(&mut self, tab: &str) -> bool {
        remove_tab(self, tab)
    }
}

/// Walk the tree, remove the first leaf-entry matching `tab_id`. If the
/// owning leaf becomes empty, collapse the surrounding Split by replacing
/// it with the surviving sibling. Returns true if something was removed.
fn remove_tab(node: &mut DockNode, tab_id: &str) -> bool {
    match node {
        DockNode::Leaf(leaf) => {
            if let Some(i) = leaf.tabs.iter().position(|t| t == tab_id) {
                leaf.tabs.remove(i);
                if leaf.active >= leaf.tabs.len() && !leaf.tabs.is_empty() {
                    leaf.active = leaf.tabs.len() - 1;
                }
                return true;
            }
            false
        }
        DockNode::Split(_) => {
            // Try first; if first becomes empty, collapse.
            let removed_first = {
                if let DockNode::Split(s) = node {
                    remove_tab(&mut s.first, tab_id)
                } else {
                    false
                }
            };
            if removed_first && is_empty_leaf(node_first(node)) {
                collapse_to(node, true);
                return true;
            }
            if removed_first {
                return true;
            }
            let removed_second = {
                if let DockNode::Split(s) = node {
                    remove_tab(&mut s.second, tab_id)
                } else {
                    false
                }
            };
            if removed_second && is_empty_leaf(node_second(node)) {
                collapse_to(node, false);
            }
            removed_second
        }
    }
}

fn node_first(n: &DockNode) -> &DockNode {
    match n {
        DockNode::Split(s) => &s.first,
        DockNode::Leaf(_) => n,
    }
}
fn node_second(n: &DockNode) -> &DockNode {
    match n {
        DockNode::Split(s) => &s.second,
        DockNode::Leaf(_) => n,
    }
}

fn is_empty_leaf(n: &DockNode) -> bool {
    matches!(n, DockNode::Leaf(l) if l.tabs.is_empty())
}

/// Replace a Split node with one of its children — used after removing
/// a tab that emptied a leaf. `first_emptied = true` means the first
/// child is the empty one and we keep the second.
fn collapse_to(node: &mut DockNode, first_emptied: bool) {
    if let DockNode::Split(s) = node {
        let survivor = if first_emptied {
            std::mem::replace(
                &mut *s.second,
                DockNode::Leaf(Leaf::new(vec![], 0)),
            )
        } else {
            std::mem::replace(
                &mut *s.first,
                DockNode::Leaf(Leaf::new(vec![], 0)),
            )
        };
        *node = survivor;
    }
}

/// Find the leaf that contains `anchor` and insert `dragged` according
/// to `zone` (Center = add to its tabs; N/S/E/W = create a new Split).
fn insert_at(
    node: &mut DockNode,
    anchor: &str,
    dragged: &str,
    zone: DropZone,
    at: Option<usize>,
) -> bool {
    match node {
        DockNode::Leaf(leaf) => {
            if leaf.tabs.iter().any(|t| t == anchor) {
                apply_zone(node, dragged, zone, at);
                true
            } else {
                false
            }
        }
        DockNode::Split(_) => {
            // recurse first
            {
                if let DockNode::Split(s) = node {
                    if insert_at(&mut s.first, anchor, dragged, zone, at) {
                        return true;
                    }
                }
            }
            if let DockNode::Split(s) = node {
                insert_at(&mut s.second, anchor, dragged, zone, at)
            } else {
                false
            }
        }
    }
}

fn apply_zone(node: &mut DockNode, dragged: &str, zone: DropZone, at: Option<usize>) {
    match zone {
        DropZone::Center => {
            if let DockNode::Leaf(leaf) = node {
                // `at` chooses the slot among the existing tabs (set from the
                // drop position in the tab bar); `None` appends at the end.
                let idx = at.unwrap_or(leaf.tabs.len()).min(leaf.tabs.len());
                leaf.tabs.insert(idx, dragged.to_string());
                leaf.active = idx;
                // Docking into a hidden strip reveals it — the user needs to
                // see where the tab landed.
                leaf.tabs_hidden = false;
            }
        }
        DropZone::North | DropZone::South | DropZone::East | DropZone::West => {
            // Wrap the existing leaf in a new Split, with the dragged
            // tab on the appropriate side.
            let dragged_leaf = DockNode::Leaf(Leaf::new(vec![dragged.to_string()], 0));
            let existing = std::mem::replace(
                node,
                DockNode::Leaf(Leaf::new(vec![], 0)),
            );
            *node = match zone {
                DropZone::North => DockNode::split_v(0.5, dragged_leaf, existing),
                DropZone::South => DockNode::split_v(0.5, existing, dragged_leaf),
                DropZone::West => DockNode::split_h(0.5, dragged_leaf, existing),
                DropZone::East => DockNode::split_h(0.5, existing, dragged_leaf),
                DropZone::Center => unreachable!(),
            };
        }
    }
}

/// Borrow the leaf that contains `anchor`, for measuring its tab strip.
fn find_leaf<'a>(node: &'a DockNode, anchor: &str) -> Option<&'a Leaf> {
    match node {
        DockNode::Leaf(leaf) => leaf.tabs.iter().any(|t| t == anchor).then_some(leaf),
        DockNode::Split(s) => {
            find_leaf(&s.first, anchor).or_else(|| find_leaf(&s.second, anchor))
        }
    }
}

/// Where an external tab would land if dropped on `leaf`'s tab bar at cursor x
/// `px`: the insertion index (`0..=tabs.len()`) and the caret x to draw. Mirrors
/// `draw_tabs` geometry so the caret falls on real tab boundaries.
fn tab_drop_index(
    leaf: &Leaf,
    leaf_rect: Rect,
    px: f32,
    text: &mut TextRenderer,
    font: f32,
    opts: &LayoutOpts,
) -> (usize, f32) {
    let mut cx = leaf_rect.min.x + 6.0;
    for (i, tab) in leaf.tabs.iter().enumerate() {
        let w = tab_width(text, opts.title(tab), font, opts);
        if px < cx + w * 0.5 {
            return (i, cx - 2.0);
        }
        cx += w + 4.0;
    }
    (leaf.tabs.len(), cx - 2.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    Color::rgba(
        lerp(a.r, b.r, t),
        lerp(a.g, b.g, t),
        lerp(a.b, b.b, t),
        lerp(a.a, b.a, t),
    )
}
