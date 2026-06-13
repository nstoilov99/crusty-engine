use crate::context::{Context, Direction, Ui, UiOptions};
use crate::dock::tree::{DockNode, Leaf, SplitDirection, TabId};
use crate::id::Id;
use crate::math::{Color, Pos2, Rect, Rounding, Vec2};
use crate::paint::{PaintCmd, RectShape};
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
    /// Computed each frame during recursion: which leaf the pointer is
    /// over and which drop zone, plus an anchor tab id we use later to
    /// re-find the target leaf after any tree mutation.
    candidate: Option<DropCandidate>,
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

pub struct DockArea<'a> {
    root: &'a mut DockNode,
    state: &'a mut DockState,
}

impl<'a> DockArea<'a> {
    pub fn new(root: &'a mut DockNode, state: &'a mut DockState) -> Self {
        Self { root, state }
    }

    pub fn show<F>(
        self,
        ctx: &mut Context,
        text: &mut TextRenderer,
        rect: Rect,
        mut f: F,
    ) where
        F: FnMut(&mut Ui, &str),
    {
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
            // Reset candidate each frame; recursion will refill it.
            drag.candidate = None;
        }

        // ── Recursion: lay out and render chrome + content.
        let dock_root = Id::ROOT.with("dock");
        show_node(self.root, rect, ctx, text, &mut f, dock_root, self.state);

        // ── Post-recursion: compass + preview overlay (only if a drag
        //    is active).
        if let Some(drag) = self.state.drag.as_ref() {
            if drag.activated {
                if let Some(candidate) = &drag.candidate {
                    render_drop_overlay(ctx, candidate);
                }
                render_drag_ghost(ctx, text, drag);
            }
        }

        // ── Resolve drop / cancel on release.
        let pointer_released = ctx.input.pointer_released;
        if pointer_released {
            if let Some(drag) = self.state.drag.take() {
                if drag.activated {
                    if let Some(candidate) = &drag.candidate {
                        if candidate.zone.is_some() {
                            apply_drop(self.root, &drag.tab_id, candidate);
                        }
                    }
                }
            }
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

    draw_tabs(ctx, text, tab_bar_rect, &mut leaf.tabs, &mut leaf.active, state);

    // Record drop candidate if a drag is in flight and the pointer is
    // over this leaf. We always set the candidate when over a panel so
    // the compass can render; the `zone` field stays `None` if the
    // pointer isn't over a specific compass button or the tab bar,
    // which makes release a no-op.
    if let Some(drag) = state.drag.as_mut() {
        if drag.activated {
            if let Some(p) = ctx.input.pointer_pos {
                if rect.contains(p) {
                    let is_self_only_tab =
                        leaf.tabs.len() == 1 && leaf.tabs[0] == drag.tab_id;
                    if !is_self_only_tab {
                        let anchor = leaf
                            .tabs
                            .iter()
                            .find(|t| *t != &drag.tab_id)
                            .cloned()
                            .unwrap_or_else(|| leaf.tabs[0].clone());

                        let in_tab_bar = tab_bar_rect.contains(p);
                        let (zone, via_tab_bar) = if in_tab_bar {
                            (Some(DropZone::Center), true)
                        } else {
                            (compute_drop_zone_compass(rect, p), false)
                        };
                        drag.candidate = Some(DropCandidate {
                            target_anchor: anchor,
                            target_rect: rect,
                            zone,
                            via_tab_bar,
                        });
                    }
                }
            }
        }
    }

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
    let mut ui = Ui::with_root(ctx, text, content_rect, Direction::TopDown, opts, leaf_root_id);
    f(&mut ui, &active_tab);
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
                    candidate: None,
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
        // Drop on tab bar = add as new tab to this leaf — highlight the
        // tab bar strip so the user sees that's the target.
        let tab_bar_h = style.fonts.body + 10.0;
        let tab_bar_rect = Rect::from_min_max(
            leaf_rect.min,
            Pos2::new(leaf_rect.max.x, leaf_rect.min.y + tab_bar_h),
        );
        let mut tint = style.palette.accent;
        tint.a = 0.55;
        ctx.paint.push(PaintCmd::Rrect(
            RectShape::fill(tab_bar_rect, style.rounding.small, tint)
                .with_stroke(1.5, style.palette.accent),
        ));
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

fn render_drag_ghost(ctx: &mut Context, text: &mut TextRenderer, drag: &DragState) {
    let style = ctx.style;
    let label_size = text.measure(&drag.tab_id, style.fonts.body, None);
    let pad = Vec2::new(10.0, 6.0);
    let size = Vec2::new(label_size.x + pad.x * 2.0, label_size.y + pad.y * 2.0);
    // Position the ghost slightly offset from the pointer so it doesn't
    // sit under the cursor.
    let pos = Pos2::new(drag.pointer_now.x + 8.0, drag.pointer_now.y - size.y * 0.5);
    let rect = Rect::from_min_size(pos, size);
    let mut fill = style.palette.accent;
    fill.a = 0.85;
    let shape = RectShape::fill(rect, style.rounding.small, fill)
        .with_stroke(1.0, style.palette.stroke);
    ctx.paint.push(PaintCmd::Rrect(shape));
    let text_pos = Pos2::new(rect.min.x + pad.x, rect.min.y + pad.y);
    text.add_text(
        &mut ctx.paint,
        &drag.tab_id,
        style.fonts.body,
        text_pos,
        Color::rgba(0.05, 0.05, 0.07, 0.95),
        None,
    );
}

// ─── tree mutation ──────────────────────────────────────────────────────────

fn apply_drop(root: &mut DockNode, dragged: &str, candidate: &DropCandidate) {
    let Some(zone) = candidate.zone else { return; };
    // Refuse the no-op cases: dropping the only tab onto its own leaf.
    if leaf_contains_only(root, dragged, &candidate.target_anchor) {
        return;
    }
    // Step 1: detach the dragged tab from its current home.
    if !remove_tab(root, dragged) {
        return;
    }
    // Step 2: insert at the target leaf (re-found via anchor in case
    // step 1 collapsed splits).
    insert_at(root, &candidate.target_anchor, dragged, zone);
}

fn leaf_contains_only(node: &DockNode, tab_id: &str, anchor: &str) -> bool {
    match node {
        DockNode::Leaf(leaf) => leaf.tabs.iter().any(|t| t == tab_id) && tab_id == anchor,
        DockNode::Split(s) => {
            leaf_contains_only(&s.first, tab_id, anchor)
                || leaf_contains_only(&s.second, tab_id, anchor)
        }
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
fn insert_at(node: &mut DockNode, anchor: &str, dragged: &str, zone: DropZone) -> bool {
    match node {
        DockNode::Leaf(leaf) => {
            if leaf.tabs.iter().any(|t| t == anchor) {
                apply_zone(node, dragged, zone);
                true
            } else {
                false
            }
        }
        DockNode::Split(_) => {
            // recurse first
            {
                if let DockNode::Split(s) = node {
                    if insert_at(&mut s.first, anchor, dragged, zone) {
                        return true;
                    }
                }
            }
            if let DockNode::Split(s) = node {
                insert_at(&mut s.second, anchor, dragged, zone)
            } else {
                false
            }
        }
    }
}

fn apply_zone(node: &mut DockNode, dragged: &str, zone: DropZone) {
    match zone {
        DropZone::Center => {
            if let DockNode::Leaf(leaf) = node {
                leaf.tabs.push(dragged.to_string());
                leaf.active = leaf.tabs.len() - 1;
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
