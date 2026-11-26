use crate::context::{Direction, Ui, UiOptions};
use crate::id::Id;
use crate::math::{Pos2, Rect, Vec2};

/// A column-aligned layout for label/value rows — the inspector's
/// workhorse. Columns size to the widest cell seen, with the measured
/// widths persisted in [`Memory`](crate::context::Memory)'s typed store so
/// alignment is stable frame-to-frame (the first frame a column appears it
/// uses `min_col_width`, then snaps to content).
///
/// ```ignore
/// Grid::new("entity").show(ui, |g| {
///     g.cell(|ui| { ui.label("Name"); });
///     g.cell(|ui| { ui.text_edit(&mut name); });
///     g.end_row();
///     g.cell(|ui| { ui.label("Visible"); });
///     g.cell(|ui| { ui.checkbox(&mut visible, ""); });
///     g.end_row();
/// });
/// ```
pub struct Grid {
    id_source: String,
    spacing: Vec2,
    min_col_width: f32,
}

impl Grid {
    pub fn new(id_source: impl Into<String>) -> Self {
        Self {
            id_source: id_source.into(),
            spacing: Vec2::new(8.0, 4.0),
            min_col_width: 0.0,
        }
    }

    /// Horizontal gap between columns and vertical gap between rows.
    pub fn spacing(mut self, spacing: Vec2) -> Self {
        self.spacing = spacing;
        self
    }

    /// Minimum width every column is given before it has measured content.
    pub fn min_col_width(mut self, w: f32) -> Self {
        self.min_col_width = w;
        self
    }

    pub fn show<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut GridUi) -> R) -> R {
        let grid_id = ui.alloc_id(("grid", &self.id_source));
        let prev_widths: Vec<f32> = ui
            .ctx()
            .memory
            .data_get::<Vec<f32>>(grid_id)
            .cloned()
            .unwrap_or_default();
        let origin = ui.cursor();

        let mut grid = GridUi {
            ui,
            grid_id,
            origin,
            spacing: self.spacing,
            min_col_width: self.min_col_width,
            prev_widths,
            col_widths: Vec::new(),
            col: 0,
            row: 0,
            cur_x: origin.x,
            row_y: origin.y,
            row_height: 0.0,
            max_x: origin.x,
        };
        let result = add_contents(&mut grid);
        grid.finish();
        result
    }
}

/// Handle passed to a [`Grid`]'s closure. Call [`cell`](Self::cell) once per
/// column then [`end_row`](Self::end_row) to move down.
pub struct GridUi<'ui, 'p> {
    ui: &'p mut Ui<'ui>,
    grid_id: Id,
    origin: Pos2,
    spacing: Vec2,
    min_col_width: f32,
    /// Column widths measured last frame; drives this frame's alignment.
    prev_widths: Vec<f32>,
    /// Column widths measured this frame; written back on `finish`.
    col_widths: Vec<f32>,
    col: usize,
    row: usize,
    cur_x: f32,
    row_y: f32,
    row_height: f32,
    max_x: f32,
}

impl GridUi<'_, '_> {
    /// Lay out one cell in the current row. The cell is given the column's
    /// width (from last frame) and unbounded height; its measured size feeds
    /// next frame's column sizing.
    pub fn cell<R>(&mut self, f: impl FnOnce(&mut Ui) -> R) -> R {
        let col = self.col;
        // On a column's first-ever frame we have no measured width yet.
        // Constraining the cell to `0` (or `min_col_width`) would make text
        // wrap to nothing — one glyph per line — and the bad measurement
        // would persist, collapsing the column forever. So measure the
        // first frame at an effectively unbounded width; the natural size
        // is stored and constrains/aligns every later frame.
        let known = self.prev_widths.get(col).copied();
        let col_width = known.map(|w| w.max(self.min_col_width)).unwrap_or(1.0e4);

        let rect = Rect::from_min_size(
            Pos2::new(self.cur_x, self.row_y),
            Vec2::new(col_width, 1.0e6),
        );
        let cell_root = self.grid_id.with((self.row as u64, col as u64));
        let opts = UiOptions {
            padding: Vec2::ZERO,
            spacing: self.spacing.y,
        };
        let (result, used) = self.ui.run_at(rect, Direction::TopDown, cell_root, opts, f);

        let used_w = used.size().x.max(self.min_col_width);
        if self.col_widths.len() <= col {
            self.col_widths.resize(col + 1, 0.0);
        }
        self.col_widths[col] = self.col_widths[col].max(used_w);
        self.row_height = self.row_height.max(used.size().y);

        // Advance by the allotted column width once it's known, else by the
        // width we just measured so a first frame doesn't blow the layout
        // out to the unbounded measuring width.
        let advance = known.map(|w| w.max(self.min_col_width)).unwrap_or(used_w);
        self.cur_x += advance + self.spacing.x;
        self.max_x = self.max_x.max(self.cur_x - self.spacing.x);
        self.col += 1;
        result
    }

    /// Finish the current row and start a new one below it.
    pub fn end_row(&mut self) {
        self.row_y += self.row_height + self.spacing.y;
        self.cur_x = self.origin.x;
        self.col = 0;
        self.row += 1;
        self.row_height = 0.0;
    }

    fn finish(&mut self) {
        // Account for a trailing row the caller didn't `end_row`.
        if self.row_height > 0.0 {
            self.row_y += self.row_height;
        }
        let widths = std::mem::take(&mut self.col_widths);
        self.ui
            .ctx_mut()
            .memory
            .data_insert::<Vec<f32>>(self.grid_id, widths);

        let total = Vec2::new(
            (self.max_x - self.origin.x).max(0.0),
            (self.row_y - self.origin.y).max(0.0),
        );
        if total.x > 0.0 && total.y > 0.0 {
            let _ = self.ui.allocate(total);
        }
    }
}
