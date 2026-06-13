//! Built-in widgets. Phase 8 adds [`ComboBox`] + [`SelectableValue`], a
//! free-function tooltip helper, and a free-function context-menu
//! helper, all on top of the Phase 3–7 widget set.

mod area;
mod button;
mod checkbox;
mod collapsing;
mod combo_box;
mod context_menu;
mod label;
mod progress;
mod radio;
mod scroll_area;
mod selectable;
mod slider;
mod text_edit;
mod tooltip;
mod tree;
mod window;

pub use area::Area;
pub use button::Button;
pub use checkbox::Checkbox;
pub use collapsing::CollapsingHeader;
pub use combo_box::ComboBox;
pub use context_menu::context_menu_for;
pub use label::Label;
pub use progress::ProgressBar;
pub use radio::Radio;
pub use scroll_area::ScrollArea;
pub use selectable::SelectableValue;
pub use slider::Slider;
pub use text_edit::TextEdit;
pub use tooltip::show_tooltip_for;
pub use tree::TreeNode;
pub use window::{Window, WindowResponse};

use std::ops::RangeInclusive;

use crate::context::Ui;
use crate::math::Rect;
use crate::response::Response;

impl Ui<'_> {
    pub fn label(&mut self, text: impl Into<String>) -> Response {
        Label::new(text).show(self)
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

    pub fn progress_bar(&mut self, progress: f32) -> Response {
        ProgressBar::new(progress).show(self)
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
        T: PartialEq + Copy,
    {
        TreeNode::new(selection, value, label).show(self, body)
    }
}
