use crate::context::{Direction, Ui, UiOptions};
use crate::id::Id;
use crate::math::{Color, Pos2, Rect, Vec2};
use crate::paint::PaintCmd;

/// Sort direction reported for a sortable column.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortDir {
    Ascending,
    Descending,
}

/// Specification for a single [`Table`] column.
pub struct Column {
    header: String,
    width: f32,
    min_width: f32,
    resizable: bool,
    sortable: bool,
}

impl Column {
    /// A column with the given header and initial width (pixels).
    pub fn new(header: impl Into<String>, width: f32) -> Self {
        Self {
            header: header.into(),
            width,
            min_width: 24.0,
            resizable: false,
            sortable: false,
        }
    }

    /// Allow the user to drag this column's right border to resize it.
    pub fn resizable(mut self, v: bool) -> Self {
        self.resizable = v;
        self
    }

    /// Allow clicking this column's header to sort by it.
    pub fn sortable(mut self, v: bool) -> Self {
        self.sortable = v;
        self
    }

    /// Smallest width the column can be dragged to.
    pub fn min_width(mut self, w: f32) -> Self {
        self.min_width = w;
        self
    }
}

/// Persistent table state kept in the typed [`Memory`](crate::context::Memory)
/// store: live column widths (so resizes survive frames), the active sort,
/// and the vertical scroll offset of the virtualized body.
#[derive(Clone, Debug, Default)]
struct TableState {
    widths: Vec<f32>,
    sort_col: Option<usize>,
    sort_asc: bool,
    offset_y: f32,
}

/// A column-based data table with optionally resizable / sortable columns
/// and a vertically virtualized body — only rows intersecting the viewport
/// are built each frame, so very long tables (the profiler) stay cheap.
///
/// ```ignore
/// let out = Table::new("entities")
///     .row_height(20.0)
///     .max_height(240.0)
///     .column(Column::new("Name", 160.0).resizable(true).sortable(true))
///     .column(Column::new("Kind", 100.0).sortable(true))
///     .show(ui, |body| {
///         if let Some((col, dir)) = body.sort() { /* sort your data */ }
///         body.rows(rows.len(), |i, row| {
///             row.cell(|ui| { ui.label(&rows[i].name); });
///             row.cell(|ui| { ui.label(&rows[i].kind); });
///         });
///     });
/// ```
pub struct Table {
    id_source: String,
    columns: Vec<Column>,
    row_height: f32,
    max_height: f32,
    striped: bool,
}

/// Result of [`Table::show`] — the active sort, also handed to the body
/// closure via [`TableBody::sort`].
#[derive(Clone, Copy, Debug)]
pub struct TableOutput {
    pub sort: Option<(usize, SortDir)>,
}

impl Table {
    pub fn new(id_source: impl Into<String>) -> Self {
        Self {
            id_source: id_source.into(),
            columns: Vec::new(),
            row_height: 22.0,
            max_height: 240.0,
            striped: true,
        }
    }

    pub fn column(mut self, col: Column) -> Self {
        self.columns.push(col);
        self
    }

    /// Height of every body row (drives virtualization math).
    pub fn row_height(mut self, h: f32) -> Self {
        self.row_height = h;
        self
    }

    /// Maximum body height before the rows scroll.
    pub fn max_height(mut self, h: f32) -> Self {
        self.max_height = h;
        self
    }

    pub fn striped(mut self, v: bool) -> Self {
        self.striped = v;
        self
    }

    pub fn show(self, ui: &mut Ui, add_body: impl FnOnce(&mut TableBody)) -> TableOutput {
        let style = ui.style();
        let table_id = ui.alloc_id(("table", &self.id_source));

        // Load (or seed) persistent state.
        let mut state = ui
            .ctx()
            .memory
            .data_get::<TableState>(table_id)
            .cloned()
            .unwrap_or_default();
        if state.widths.len() != self.columns.len() {
            state.widths = self.columns.iter().map(|c| c.width).collect();
        }

        let origin = ui.cursor();
        let header_h = self.row_height.max(style.fonts.body + 8.0);

        // Column left edges from current widths.
        let mut col_left = Vec::with_capacity(self.columns.len() + 1);
        let mut x = origin.x;
        for w in &state.widths {
            col_left.push(x);
            x += *w;
        }
        col_left.push(x);
        let total_w = x - origin.x;

        let header_rect = Rect::from_min_size(origin, Vec2::new(total_w, header_h));
        let _ = ui.allocate(Vec2::new(total_w, header_h));

        // Header background.
        ui.ctx_mut().paint.push(PaintCmd::Rect {
            rect: header_rect,
            color: style.palette.active,
        });

        let pointer_pos = ui.ctx().input.pointer_pos;
        let pointer_pressed = ui.ctx().input.pointer_pressed;
        let pointer_released = ui.ctx().input.pointer_released;
        let pointer_down = ui.ctx().input.pointer_down;

        // --- Column resize handles (right border of each resizable column). ---
        let mut resizing_any = false;
        for (i, col) in self.columns.iter().enumerate() {
            if !col.resizable {
                continue;
            }
            let border_x = col_left[i + 1];
            let handle = Rect::from_min_size(
                Pos2::new(border_x - 3.0, origin.y),
                Vec2::new(6.0, header_h),
            );
            let handle_id = table_id.with(("resize", i as u64));
            let hovered = ui.contains_pointer(handle);
            let active = ui.ctx().memory.active_widget;
            if pointer_pressed && hovered {
                ui.ctx_mut().memory.active_widget = Some(handle_id);
            }
            let is_active = active == Some(handle_id);
            if is_active && pointer_down {
                resizing_any = true;
                if let Some(p) = pointer_pos {
                    let new_w = (p.x - col_left[i]).max(col.min_width);
                    state.widths[i] = new_w;
                }
            }
            if is_active && pointer_released {
                ui.ctx_mut().memory.active_widget = None;
            }
            if hovered || is_active {
                ui.ctx_mut().paint.push(PaintCmd::Rect {
                    rect: Rect::from_min_size(
                        Pos2::new(border_x - 1.0, origin.y),
                        Vec2::new(2.0, header_h),
                    ),
                    color: style.palette.accent_active,
                });
            }
        }

        // Recompute edges in case a resize changed a width this frame.
        col_left.clear();
        let mut x = origin.x;
        for w in &state.widths {
            col_left.push(x);
            x += *w;
        }
        col_left.push(x);
        let total_w = x - origin.x;

        // --- Header labels + sort handling. ---
        for (i, col) in self.columns.iter().enumerate() {
            let cell = Rect::from_min_size(
                Pos2::new(col_left[i], origin.y),
                Vec2::new(state.widths[i], header_h),
            );
            // Sort click (ignore the click if we were dragging a border).
            if col.sortable && !resizing_any {
                let hovered = ui.contains_pointer(cell);
                if pointer_released && hovered {
                    if state.sort_col == Some(i) {
                        state.sort_asc = !state.sort_asc;
                    } else {
                        state.sort_col = Some(i);
                        state.sort_asc = true;
                    }
                }
            }

            ui.ctx_mut().paint.push(PaintCmd::PushClip(cell));
            let text_pos = Pos2::new(cell.min.x + 6.0, cell.min.y + (header_h - style.fonts.body) * 0.5);
            {
                let Ui { ctx, text, .. } = ui;
                text.add_text(
                    &mut ctx.paint,
                    &col.header,
                    style.fonts.body,
                    text_pos,
                    style.palette.text,
                    None,
                );
            }
            // Sort indicator: an up chevron (^) for ascending, a down
            // chevron (v) for descending.
            if state.sort_col == Some(i) {
                let half = style.fonts.small * 0.28;
                let width = (style.fonts.small * 0.12).max(1.5);
                let color = style.palette.text_secondary;
                let cx = cell.max.x - 9.0;
                let cy = cell.min.y + header_h * 0.5;
                // Outer arms sit on one side of `cy`, the tip on the other.
                let (arm_y, tip_y) = if state.sort_asc {
                    (cy + half * 0.5, cy - half * 0.5)
                } else {
                    (cy - half * 0.5, cy + half * 0.5)
                };
                let left = Pos2::new(cx - half, arm_y);
                let mid = Pos2::new(cx, tip_y);
                let right = Pos2::new(cx + half, arm_y);
                let paint = &mut ui.ctx_mut().paint;
                paint.push(PaintCmd::Line { from: left, to: mid, width, color });
                paint.push(PaintCmd::Line { from: mid, to: right, width, color });
            }
            ui.ctx_mut().paint.push(PaintCmd::PopClip);
        }

        // --- Virtualized body. ---
        let mut body = TableBody {
            ui,
            table_id,
            origin: Pos2::new(origin.x, origin.y + header_h),
            col_left,
            widths: state.widths.clone(),
            total_w,
            row_height: self.row_height,
            max_height: self.max_height,
            striped: self.striped,
            stripe_color: {
                let mut c = style.palette.hover;
                c.a *= 0.5;
                c
            },
            sort: state.sort_col.map(|c| {
                (
                    c,
                    if state.sort_asc {
                        SortDir::Ascending
                    } else {
                        SortDir::Descending
                    },
                )
            }),
            offset_y: state.offset_y,
        };
        add_body(&mut body);
        state.offset_y = body.offset_y;

        let sort = body.sort;
        ui.ctx_mut().memory.data_insert::<TableState>(table_id, state);
        TableOutput { sort }
    }
}

/// Handle passed to a [`Table`]'s body closure. Read [`sort`](Self::sort)
/// to order your data, then call [`rows`](Self::rows) once with the row
/// count; only visible rows are actually built.
pub struct TableBody<'ui, 'p> {
    ui: &'p mut Ui<'ui>,
    table_id: Id,
    origin: Pos2,
    col_left: Vec<f32>,
    widths: Vec<f32>,
    total_w: f32,
    row_height: f32,
    max_height: f32,
    striped: bool,
    stripe_color: Color,
    sort: Option<(usize, SortDir)>,
    offset_y: f32,
}

impl TableBody<'_, '_> {
    /// The active sort, if any — apply it to your data before `rows`.
    pub fn sort(&self) -> Option<(usize, SortDir)> {
        self.sort
    }

    /// Build `num_rows` rows, virtualized: `add_row` is only invoked for the
    /// rows currently intersecting the (scrolled) viewport.
    pub fn rows(&mut self, num_rows: usize, mut add_row: impl FnMut(usize, &mut TableRow)) {
        let style = self.ui.style();
        let total_h = num_rows as f32 * self.row_height;
        let body_h = total_h.min(self.max_height);
        let outer = Rect::from_min_size(self.origin, Vec2::new(self.total_w, body_h));
        let _ = self.ui.allocate(Vec2::new(self.total_w, body_h));

        // Scroll handling.
        let max_offset = (total_h - body_h).max(0.0);
        let mut offset = self.offset_y.clamp(0.0, max_offset);
        if self.ui.contains_pointer(outer) {
            let wheel = self.ui.ctx().input.scroll_delta.y;
            if wheel != 0.0 {
                offset = (offset - wheel * 30.0).clamp(0.0, max_offset);
            }
        }
        self.offset_y = offset;

        self.ui.ctx_mut().paint.push(PaintCmd::PushClip(outer));

        // Visible row range.
        let first = (offset / self.row_height).floor() as usize;
        let last = (((offset + body_h) / self.row_height).ceil() as usize).min(num_rows);

        for r in first..last {
            let row_top = outer.min.y - offset + r as f32 * self.row_height;
            let row_rect = Rect::from_min_size(
                Pos2::new(outer.min.x, row_top),
                Vec2::new(self.total_w, self.row_height),
            );
            if self.striped && r % 2 == 1 {
                self.ui.ctx_mut().paint.push(PaintCmd::Rect {
                    rect: row_rect,
                    color: self.stripe_color,
                });
            }

            let mut row = TableRow {
                ui: self.ui,
                table_id: self.table_id,
                col_left: &self.col_left,
                widths: &self.widths,
                row_index: r,
                row_top,
                row_height: self.row_height,
                col: 0,
                spacing: style.spacing.item,
            };
            add_row(r, &mut row);
        }

        self.ui.ctx_mut().paint.push(PaintCmd::PopClip);
    }
}

/// Handle for one visible table row. Call [`cell`](Self::cell) once per
/// column in order.
pub struct TableRow<'a, 'ui, 'p> {
    ui: &'p mut Ui<'ui>,
    table_id: Id,
    col_left: &'a [f32],
    widths: &'a [f32],
    row_index: usize,
    row_top: f32,
    row_height: f32,
    col: usize,
    spacing: f32,
}

impl TableRow<'_, '_, '_> {
    /// Render the next column's cell. Content is clipped to the column.
    pub fn cell<R>(&mut self, f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        let c = self.col;
        self.col += 1;
        let left = *self.col_left.get(c)?;
        let width = *self.widths.get(c)?;
        let cell_rect = Rect::from_min_size(
            Pos2::new(left, self.row_top),
            Vec2::new(width, self.row_height),
        );
        let root = self.table_id.with(("cell", self.row_index as u64, c as u64));
        let opts = UiOptions {
            padding: Vec2::new(6.0, 3.0),
            spacing: self.spacing,
        };
        self.ui.ctx_mut().paint.push(PaintCmd::PushClip(cell_rect));
        let (result, _) = self.ui.run_at(cell_rect, Direction::TopDown, root, opts, f);
        self.ui.ctx_mut().paint.push(PaintCmd::PopClip);
        Some(result)
    }
}
