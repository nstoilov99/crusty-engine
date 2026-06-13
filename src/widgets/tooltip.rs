use crate::context::Ui;
use crate::math::{Pos2, Rect, Vec2};
use crate::paint::{PaintCmd, RectShape};

/// Render a tooltip near the pointer if the pointer is currently over
/// `rect`. Pure helper — uses [`Context::overlay_paint`] so it draws
/// above everything else.
///
/// Phase 8 ships with no hover-delay: the tooltip is visible as soon as
/// the pointer is over the rect. A proper "appears after N ms" timer can
/// land in a follow-up polish phase.
pub fn show_tooltip_for(ui: &mut Ui, rect: Rect, text: &str) {
    if !ui.contains_pointer(rect) {
        return;
    }
    let Some(pointer) = ui.ctx().input.pointer_pos else {
        return;
    };
    let style = ui.style();
    let size = ui.text_mut().measure(text, style.fonts.small, None);
    let pad = Vec2::new(8.0, 4.0);
    let tip_rect = Rect::from_min_size(
        Pos2::new(pointer.x + 12.0, pointer.y + 18.0),
        Vec2::new(size.x + pad.x * 2.0, size.y + pad.y * 2.0),
    );

    ui.ctx_mut().overlay_paint.push(PaintCmd::Rrect(
        RectShape::fill(
            tip_rect,
            style.rounding.small,
            style.palette.surface_active,
        )
        .with_stroke(1.0, style.palette.stroke),
    ));
    let text_pos = Pos2::new(tip_rect.min.x + pad.x, tip_rect.min.y + pad.y);
    let Ui { ctx, text: text_renderer, .. } = ui;
    text_renderer.add_text(
        &mut ctx.overlay_paint,
        text,
        style.fonts.small,
        text_pos,
        style.palette.text,
        None,
    );
}
