use crate::context::{Direction, Ui, UiOptions};
use crate::id::Id;
use crate::layer::Order;
use crate::math::{Pos2, Rect, Vec2};

/// A free-positioned floating region — the primitive that [`Window`]
/// (and future floating containers) build on.
///
/// An `Area`:
/// * paints into its own layer, z-sorted against other areas,
/// * remembers its position across frames,
/// * is brought to the front when clicked,
/// * sizes itself to its content.
///
/// [`Window`]: crate::widgets::Window
pub struct Area {
    id: Id,
    order: Order,
    default_pos: Pos2,
    interactable: bool,
    constrain: bool,
}

impl Area {
    pub fn new(id_source: impl std::hash::Hash) -> Self {
        Self {
            id: Id::new(("area", id_source)),
            order: Order::Middle,
            default_pos: Pos2::new(120.0, 120.0),
            interactable: true,
            constrain: true,
        }
    }

    pub fn id(&self) -> Id {
        self.id
    }

    pub fn order(mut self, order: Order) -> Self {
        self.order = order;
        self
    }

    pub fn default_pos(mut self, pos: Pos2) -> Self {
        self.default_pos = pos;
        self
    }

    /// Non-interactable areas (e.g. pure tooltips) never claim the
    /// pointer, so UI beneath them stays clickable.
    pub fn interactable(mut self, v: bool) -> Self {
        self.interactable = v;
        self
    }

    /// Keep the area inside the screen rect (default true).
    pub fn constrain(mut self, v: bool) -> Self {
        self.constrain = v;
        self
    }

    pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
        let id = self.id;
        let screen = ui.ctx().screen_rect();

        let pos = {
            let area = ui
                .ctx_mut()
                .memory
                .area_entry(id, self.default_pos, self.order);
            area.interactable = self.interactable;
            area.pos
        };

        if ui.ctx().input.pointer_pressed && ui.ctx().hovered_area() == Some(id) {
            ui.ctx_mut().memory.bring_area_to_front(id);
        }

        ui.ctx_mut().begin_layer();
        let max_rect = Rect::from_min_max(
            pos,
            Pos2::new(screen.max.x.max(pos.x + 1.0), screen.max.y.max(pos.y + 1.0)),
        );
        let (result, used) = {
            let Ui { ctx, text, .. } = ui;
            let mut inner = Ui::with_root(
                ctx,
                text,
                max_rect,
                Direction::TopDown,
                UiOptions::default(),
                id.with("ui"),
            );
            inner.set_area(Some(id));
            let result = body(&mut inner);
            (result, inner.used_rect())
        };
        ui.ctx_mut().end_layer(id, self.order);

        // Persist this frame's footprint for next frame's hit testing.
        let padding = UiOptions::default().padding;
        let mut size = (used.max - pos) + padding;
        size = size.max(Vec2::splat(1.0));
        let mut new_pos = pos;
        if self.constrain && screen.width() > 0.0 {
            new_pos.x = new_pos
                .x
                .clamp(screen.min.x, (screen.max.x - size.x).max(screen.min.x));
            new_pos.y = new_pos
                .y
                .clamp(screen.min.y, (screen.max.y - size.y).max(screen.min.y));
        }
        let area = ui
            .ctx_mut()
            .memory
            .area_entry(id, self.default_pos, self.order);
        area.pos = new_pos;
        area.size = size;

        result
    }
}
