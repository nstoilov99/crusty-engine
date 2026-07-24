//! Built-in widgets. Phase 8 adds [`ComboBox`] + [`SelectableValue`], a
//! free-function tooltip helper, and a free-function context-menu
//! helper, all on top of the Phase 3–7 widget set.

mod area;
mod button;
mod checkbox;
mod chip;
mod collapsing;
mod color_picker;
mod combo_box;
mod context_menu;
mod drag_value;
mod grid;
mod image;
mod label;
mod menu;
mod popup;
mod progress;
mod radio;
mod scroll_area;
mod selectable;
mod slider;
mod splitter;
mod table;
mod text_edit;
mod toast;
mod toggle;
mod tooltip;
mod tree;
mod window;

pub use area::Area;
pub use button::{Button, ButtonVariant};
pub use checkbox::Checkbox;
pub use chip::{CountChip, FilterChip};
pub use collapsing::CollapsingHeader;
pub use color_picker::{ColorPicker, EyedropperState};
pub use combo_box::ComboBox;
pub use context_menu::context_menu_for;
pub use drag_value::DragValue;
pub use grid::{Grid, GridUi};
pub use image::{Image, ImageButton};
pub use label::Label;
pub use popup::Popup;
pub use progress::ProgressBar;
pub use radio::Radio;
pub use scroll_area::ScrollArea;
pub use selectable::SelectableValue;
pub use slider::Slider;
pub use splitter::Splitter;
pub use table::{Column, SortDir, Table, TableBody, TableOutput, TableRow};
pub use text_edit::{TextEdit, TextEditOutput};
pub use toast::ToastKind;
pub use toggle::Toggle;
pub use tooltip::{show_tooltip_for, Tooltip};
pub use tree::{TreeDropPos, TreeNode, TreeNodeOutput};
pub use window::{Window, WindowResponse};

use std::ops::RangeInclusive;

use crate::context::Ui;
use crate::math::{Color, Pos2, Rect, Vec2};
use crate::paint::{PaintCmd, TextureId};
use crate::response::Response;

/// Place a popup of `width` next to `anchor` so it stays inside the screen:
/// prefer below the anchor, flip above when there's no room, clamp as a
/// last resort. Popups are laid out before their height is known, so the
/// height comes from the popup's area size last frame (`area_id`), falling
/// back to `fallback_h` on the first open frame (one-frame correction).
pub(crate) fn popup_pos(
    ui: &Ui,
    area_id: crate::id::Id,
    anchor: Rect,
    width: f32,
    fallback_h: f32,
    gap: f32,
) -> Pos2 {
    const MARGIN: f32 = 4.0;
    let screen = ui.ctx().screen_rect();
    let h = ui
        .ctx()
        .memory
        .area(area_id)
        .map(|a| a.size.y)
        .filter(|h| *h > 0.0)
        .unwrap_or(fallback_h);
    let x = anchor
        .min
        .x
        .min(screen.max.x - width - MARGIN)
        .max(screen.min.x + MARGIN);
    let below = anchor.max.y + gap;
    let above = anchor.min.y - gap - h;
    let y = if below + h <= screen.max.y - MARGIN || above < screen.min.y + MARGIN {
        below.min((screen.max.y - MARGIN - h).max(screen.min.y + MARGIN))
    } else {
        above
    };
    Pos2::new(x, y)
}

/// Shared keyboard-focus plumbing: registers `id` as focusable in the
/// Ui's scope, claims focus when clicked, and reports Enter/Space
/// activation while focused. Call after pointer click detection and merge
/// the returned `activated` into the click handling. Returns
/// `(focused, activated)`.
pub(crate) fn focus_interact(ui: &mut Ui, id: crate::id::Id, clicked: bool) -> (bool, bool) {
    let scope = ui.area();
    ui.ctx_mut().memory.register_focusable(id, scope);
    if clicked {
        ui.ctx_mut().memory.focused_widget = Some(id);
    }
    let focused = ui.ctx().memory.focused_widget == Some(id);
    let activated = focused
        && (ui.ctx().input.key_pressed(crate::input::Key::Enter)
            || ui.ctx().input.key_pressed(crate::input::Key::Space));
    (focused, activated)
}

/// Append a disclosure chevron centered at `center`. `half` is the
/// half-extent of the glyph; `width` the stroke width. Drawn as two strokes
/// (a down "v" when `open`, a right ">" when closed) so it never depends on
/// a font's arrow glyph — which some fonts substitute with a color emoji.
pub(crate) fn paint_chevron(
    paint: &mut Vec<PaintCmd>,
    center: Pos2,
    half: f32,
    width: f32,
    open: bool,
    color: Color,
) {
    let (cx, cy) = (center.x, center.y);
    let (a, mid, b) = if open {
        (
            Pos2::new(cx - half, cy - half * 0.5),
            Pos2::new(cx, cy + half * 0.5),
            Pos2::new(cx + half, cy - half * 0.5),
        )
    } else {
        (
            Pos2::new(cx - half * 0.5, cy - half),
            Pos2::new(cx + half * 0.5, cy),
            Pos2::new(cx - half * 0.5, cy + half),
        )
    };
    paint.push(PaintCmd::Line { from: a, to: mid, width, color });
    paint.push(PaintCmd::Line { from: mid, to: b, width, color });
}

impl Ui<'_> {
    pub fn label(&mut self, text: impl Into<String>) -> Response {
        Label::new(text).show(self)
    }

    /// Label in the bundled mono family, at the style's mono size —
    /// values, paths, shortcuts, logs.
    pub fn mono_label(&mut self, text: impl Into<String>) -> Response {
        let size = self.style().fonts.mono;
        Label::new(text).mono().size(size).show(self)
    }

    pub fn button(&mut self, text: impl Into<String>) -> Response {
        Button::new(text).show(self)
    }

    pub fn checkbox(&mut self, value: &mut bool, label: impl Into<String>) -> Response {
        Checkbox::new(value, label).show(self)
    }

    pub fn radio_value<T: PartialEq + Copy>(
        &mut self,
        selection: &mut T,
        option: T,
        label: impl Into<String>,
    ) -> Response {
        Radio::new(selection, option, label).show(self)
    }

    pub fn slider(&mut self, value: &mut f32, range: RangeInclusive<f32>) -> Response {
        Slider::new(value, range).show(self)
    }

    pub fn drag_value(&mut self, value: &mut f32) -> Response {
        DragValue::new(value).show(self)
    }

    pub fn progress_bar(&mut self, progress: f32) -> Response {
        ProgressBar::new(progress).show(self)
    }

    pub fn image(&mut self, texture: TextureId, size: Vec2) -> Response {
        Image::new(texture, size).show(self)
    }

    pub fn image_button(&mut self, texture: TextureId, size: Vec2) -> Response {
        ImageButton::new(texture, size).show(self)
    }

    pub fn text_edit(&mut self, value: &mut String) -> Response {
        TextEdit::new(value).show(self)
    }

    pub fn collapsing<R>(
        &mut self,
        label: impl Into<String>,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> Response {
        CollapsingHeader::new(label).show(self, body)
    }

    pub fn scroll_area<R>(
        &mut self,
        max_height: f32,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        ScrollArea::new(max_height).show(self, body)
    }

    pub fn grid<R>(
        &mut self,
        id_source: impl Into<String>,
        body: impl FnOnce(&mut GridUi) -> R,
    ) -> R {
        Grid::new(id_source).show(self, body)
    }

    pub fn table(
        &mut self,
        builder: Table,
        body: impl FnOnce(&mut crate::widgets::TableBody),
    ) -> TableOutput {
        builder.show(self, body)
    }

    pub fn selectable_value<T: PartialEq + Copy>(
        &mut self,
        value: &mut T,
        option: T,
        label: impl Into<String>,
    ) -> Response {
        SelectableValue::new(value, option, label).show(self)
    }

    pub fn tooltip_for(&mut self, rect: Rect, text: &str) {
        show_tooltip_for(self, rect, text);
    }

    pub fn context_menu_for(
        &mut self,
        id_source: impl std::hash::Hash,
        rect: Rect,
        body: impl FnOnce(&mut Ui),
    ) {
        context_menu_for(self, id_source, rect, body);
    }

    pub fn tree_node<T, R>(
        &mut self,
        selection: &mut T,
        value: T,
        label: impl Into<String>,
        body: impl FnOnce(&mut Ui, &mut T) -> R,
    ) -> Response
    where
        T: PartialEq + Copy + 'static,
    {
        TreeNode::new(selection, value, label).show(self, body)
    }
}
