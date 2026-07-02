use crate::context::{Context, Direction, Ui, UiOptions};
use crate::dock::tree::{DockNode, Leaf, SplitDirection, TabId};
use crate::id::Id;
use crate::math::{Color, Pos2, Rect, Rounding, Vec2};
use crate::paint::{PaintCmd, RectShape, scale_alpha};
use crate::text::TextRenderer;

const SPLITTER_THICKNESS: f32 = 5.0;
const SPLITTER_HIT_PADDING: f32 = 2.0;
const DRAG_ACTIVATION_PX: f32 = 4.0;

/// Persistent dock state — drag-in-progress info that has to survive
/// across frames. Construct once in your app, pass `&mut` to
/// [`DockArea::new`] each frame.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Default)]
pub struct DockState {
    #[cfg_attr(feature = "serde", serde(skip))]
    drag: Option<DragState>,
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

/// What [`DockArea::show`] reports back to the host after a frame.
#[derive(Default)]
pub struct DockResponse {
    /// A tab was torn off the window edge this frame (already removed from
    /// the tree). Spawn an OS window for it.
    pub detached: Option<DetachedTab>,
    /// An [`ExternalDrag`] was committed into the tree this frame. The host
    /// can close the source OS window.
    pub docked: bool,
}

pub struct DockArea<'a> {
    root: &'a mut DockNode,
    state: &'a mut DockState,
    external: Option<ExternalDrag>,
    lone_tearoff: bool,
}

impl<'a> DockArea<'a> {
    pub fn new(root: &'a mut DockNode, state: &'a mut DockState) -> Self {
        Self {
            root,
            state,
            external: None,
            lone_tearoff: false,
        }
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
        show_node(self.root, rect, ctx, text, &mut f, dock_root, self.state);

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
                // Unreal-style drop semantics, with the full compass visible:
                //   • tab bar       → dock as a tab next to the others
                //   • N/S/E/W edge  → split the leaf in that direction
                //   • center / body → float (becomes its own window)
                let tab_bar_h = ctx.style.fonts.body + 10.0;
                let tab_bar_rect = Rect::from_min_max(
                    leaf_rect.min,
                    Pos2::new(leaf_rect.max.x, leaf_rect.min.y + tab_bar_h),
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
                render_drop_overlay(ctx, &candidate);
                // Over the tab bar: figure out which slot the drop lands in from
                // the cursor x and draw an insertion caret there so the position
                // is explicit before release.
                let tab_slot = if over_tab_bar {
                    find_leaf(self.root, &candidate.target_anchor).map(|leaf| {
                        tab_drop_index(leaf, leaf_rect, ext.pointer.x, text, ctx.style.fonts.body)
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
                        ctx.style.palette.accent,
                    )));
                }
                if ext.ghost {
                    render_external_decorator(
                        ctx, text, &ext.tab_id, ext.pointer, ext.grab, &mut f,
                    );
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
                    } else if let Some(edge) = button {
                        // Center button drops fall through to the float path.
                        if edge != DropZone::Center {
                            insert_at(
                                self.root,
                                &candidate.target_anchor,
                                &ext.tab_id,
                                edge,
                                None,
                            );
                            response.docked = true;
                        }
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

fn show_node<F>(
    node: &mut DockNode,
    rect: Rect,
    ctx: &mut Context,
    text: &mut TextRenderer,
    f: &mut F,
    node_id: Id,
    state: &mut DockState,
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

            show_node(&mut split.first, rect_a, ctx, text, f, node_id.with(0), state);
            show_node(&mut split.second, rect_b, ctx, text, f, node_id.with(1), state);
        }
        DockNode::Leaf(leaf) => {
            show_leaf(leaf, rect, ctx, text, f, node_id, state);
        }
    }
}

fn show_leaf<F>(
    leaf: &mut Leaf,
    rect: Rect,
    ctx: &mut Context,
    text: &mut TextRenderer,
    f: &mut F,
    node_id: Id,
    state: &mut DockState,
) where
    F: FnMut(&mut Ui, &str),
{
    let style = ctx.style;
    let tab_bar_h = style.fonts.body + 10.0;

    // Panel background.
    let panel_shape = RectShape::fill(rect, style.rounding.panel, style.palette.surface)
        .with_stroke(1.0, style.palette.stroke);
    ctx.paint.push(PaintCmd::Rrect(panel_shape));

    // Tab bar.
    let tab_bar_rect =
        Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.min.y + tab_bar_h));
    let bar_bg = RectShape::fill(
        tab_bar_rect,
        style.rounding.small,
        style.palette.surface_active,
    );
    ctx.paint.push(PaintCmd::Rrect(bar_bg));

    // Clip the tabs to their bar so a leaf with more tabs than fit its width
    // can't bleed sideways over the adjacent panel.
    ctx.paint.push(PaintCmd::PushClip(tab_bar_rect));
    draw_tabs(ctx, text, tab_bar_rect, &mut leaf.tabs, &mut leaf.active, state);
    ctx.paint.push(PaintCmd::PopClip);

    let active_tab = leaf.tabs[leaf.active].clone();
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
    ctx.paint.push(PaintCmd::PopClip);
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
    let color = lerp_color(style.palette.stroke, style.palette.accent, highlight);
    let shape = RectShape::fill(splitter_rect, Rounding::ZERO, color);
    ctx.paint.push(PaintCmd::Rrect(shape));
}

fn draw_tabs(
    ctx: &mut Context,
    text: &mut TextRenderer,
    tab_bar_rect: Rect,
    tabs: &mut Vec<TabId>,
    active: &mut usize,
    state: &mut DockState,
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
                    let w = text.measure(tab, style.fonts.body, None).x + 18.0;
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
        let label_size = text.measure(tab, style.fonts.body, None);
        let tab_w = label_size.x + 18.0;
        let tab_rect = Rect::from_min_size(
            Pos2::new(x, tab_bar_rect.min.y + 1.0),
            Vec2::new(tab_w, bar_h - 1.0),
        );

        let is_active = i == active_was;
        let hovered = ctx.pointer_over(None, tab_rect);
        let fill = if is_active {
            style.palette.surface
        } else if hovered {
            style.palette.surface_hover
        } else {
            style.palette.surface_active
        };
        let shape = RectShape::fill(tab_rect, style.rounding.small, fill);
        ctx.paint.push(PaintCmd::Rrect(shape));

        if is_active {
            let underline = Rect::from_min_max(
                Pos2::new(tab_rect.min.x + 3.0, tab_rect.max.y - 2.0),
                Pos2::new(tab_rect.max.x - 3.0, tab_rect.max.y),
            );
            ctx.paint.push(PaintCmd::Rrect(RectShape::fill(
                underline,
                Rounding::same(1.0),
                style.palette.accent,
            )));
        }

        let text_pos = Pos2::new(
            tab_rect.min.x + 9.0,
            tab_rect.min.y + (tab_rect.height() - label_size.y) * 0.5,
        );
        let label_color = if is_active {
            style.palette.text
        } else {
            style.palette.text_dim
        };
        text.add_text(
            &mut ctx.paint,
            tab,
            style.fonts.body,
            text_pos,
            label_color,
            None,
        );

        // Start a drag on press (don't switch yet — switching happens
        // on release without movement).
        if ctx.input.pointer_pressed && hovered && state.drag.is_none() {
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

        // Click without drag = switch active tab.
        if ctx.input.pointer_released && hovered {
            if let Some(drag) = state.drag.as_ref() {
                if !drag.activated && drag.tab_id == *tab {
                    *active = i;
                }
            }
        }

        x += tab_w + 4.0;
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

fn render_drop_overlay(ctx: &mut Context, candidate: &DropCandidate) {
    let style = ctx.style;
    let leaf_rect = candidate.target_rect;

    // ── 1. Preview overlay (rendered first, so the compass sits on top).
    if candidate.via_tab_bar {
        // Drop on tab bar = add as a tab here — the exact insertion slot is
        // shown by the caret drawn in the caller, so don't tint the whole bar.
    } else if let Some(zone) = candidate.zone {
        let preview = preview_rect_for_zone(leaf_rect, zone);
        let mut tint = style.palette.accent;
        tint.a = 0.22;
        let mut stroke = style.palette.accent;
        stroke.a = 0.95;
        ctx.paint.push(PaintCmd::Rrect(
            RectShape::fill(preview, style.rounding.widget, tint)
                .with_stroke(2.0, stroke),
        ));
    }

    // ── 2. Compass buttons.
    let buttons = compass_button_rects(leaf_rect);
    for (zone, btn_rect) in buttons {
        let is_active = Some(zone) == candidate.zone && !candidate.via_tab_bar;
        let mut fill = style.palette.surface_hover;
        fill.a = if is_active { 0.95 } else { 0.78 };
        let stroke_w = if is_active { 2.0 } else { 1.0 };
        let mut stroke = style.palette.accent;
        if !is_active {
            stroke.a *= 0.55;
        }
        ctx.paint.push(PaintCmd::Rrect(
            RectShape::fill(btn_rect, style.rounding.small, fill)
                .with_stroke(stroke_w, stroke),
        ));
        draw_compass_icon(ctx, btn_rect, zone, is_active);
    }
}

/// Draws a small directional indicator inside a compass button.
fn draw_compass_icon(ctx: &mut Context, btn_rect: Rect, zone: DropZone, active: bool) {
    let style = ctx.style;
    let cx = btn_rect.center();
    let half = btn_rect.width() * 0.35;
    let color = if active {
        style.palette.accent
    } else {
        style.palette.text_dim
    };
    let icon_rect = match zone {
        DropZone::Center => Rect::from_center_size(cx, Vec2::splat(half * 0.9)),
        DropZone::North => Rect::from_center_size(
            Pos2::new(cx.x, cx.y - half * 0.35),
            Vec2::new(half * 1.4, half * 0.55),
        ),
        DropZone::South => Rect::from_center_size(
            Pos2::new(cx.x, cx.y + half * 0.35),
            Vec2::new(half * 1.4, half * 0.55),
        ),
        DropZone::East => Rect::from_center_size(
            Pos2::new(cx.x + half * 0.35, cx.y),
            Vec2::new(half * 0.55, half * 1.4),
        ),
        DropZone::West => Rect::from_center_size(
            Pos2::new(cx.x - half * 0.35, cx.y),
            Vec2::new(half * 0.55, half * 1.4),
        ),
    };
    ctx.paint.push(PaintCmd::Rrect(RectShape::fill(
        icon_rect,
        Rounding::same(1.5),
        color,
    )));
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
) where
    F: FnMut(&mut Ui, &str),
{
    let style = ctx.style;
    let tab_bar_h = style.fonts.body + 10.0;
    // Place the card so the cursor sits where the title strip was grabbed.
    let origin = Pos2::new(pointer.x - grab.x, pointer.y - grab.y);
    let rect = Rect::from_min_size(origin, DECORATOR_SIZE);

    // Everything from here is faded together at the end.
    let start = ctx.paint.len();

    // Panel body + slim header bar carrying the tab name (no window chrome).
    let body = RectShape::fill(rect, style.rounding.panel, style.palette.surface)
        .with_stroke(1.0, style.palette.accent);
    ctx.paint.push(PaintCmd::Rrect(body));
    let header = Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.min.y + tab_bar_h));
    ctx.paint.push(PaintCmd::Rrect(RectShape::fill(
        header,
        style.rounding.small,
        style.palette.surface_active,
    )));
    text.add_text(
        &mut ctx.paint,
        tab_id,
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
                DockNode::Leaf(Leaf {
                    tabs: vec![],
                    active: 0,
                }),
            )
        } else {
            std::mem::replace(
                &mut *s.first,
                DockNode::Leaf(Leaf {
                    tabs: vec![],
                    active: 0,
                }),
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
            }
        }
        DropZone::North | DropZone::South | DropZone::East | DropZone::West => {
            // Wrap the existing leaf in a new Split, with the dragged
            // tab on the appropriate side.
            let dragged_leaf = DockNode::Leaf(Leaf {
                tabs: vec![dragged.to_string()],
                active: 0,
            });
            let existing = std::mem::replace(
                node,
                DockNode::Leaf(Leaf {
                    tabs: vec![],
                    active: 0,
                }),
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
) -> (usize, f32) {
    let mut cx = leaf_rect.min.x + 6.0;
    for (i, tab) in leaf.tabs.iter().enumerate() {
        let w = text.measure(tab, font, None).x + 18.0;
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
