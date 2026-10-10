// Adapted from sn-canvas `android/app/src/main/java/com/sncanvas/canvas/TableElements.kt` and
// `TableEdits.kt`, MIT, Copyright (c) 2026 JRaghavan (notice in LICENSES-3RDPARTY.md).
//! F21-FR10: tables. A pen drag outlines a grid that grows in whole cells (160 × 48 units) up
//! to 12 rows × 8 columns; a cell's text is set with the height the shell measured; rows and
//! columns are added after the target cell and removed where the target is, never just the
//! last (sn-canvas #53); a row edge drag sets that row's height. Each is one step.
use super::edits::Made;
use super::geom::{self, Pt};
use super::item::{
    CELL_WIDTH, Item, MAX_COLS, MAX_ROW_HEIGHT, MAX_ROWS, MIN_ROW_HEIGHT, Ref, Table,
};
use super::scene::Scene;
use crate::ink::BBox;

/// Rows × columns a drag of `w` × `h` makes (`TableElements.gridForDrag`).
#[must_use]
pub fn grid_for_drag(w: f64, h: f64) -> (usize, usize) {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped below
    let n = |v: f64, unit: f64, max: usize| ((v / unit).round().max(1.0) as usize).min(max);
    (n(h, MIN_ROW_HEIGHT, MAX_ROWS), n(w, CELL_WIDTH, MAX_COLS))
}

/// A table change, as the Table actions send it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TableEdit {
    AddRow(usize),
    AddColumn(usize),
    RemoveRow(usize),
    RemoveColumn(usize),
    ResizeRow(usize, f64),
}

fn refit(it: &mut Item) {
    if let Some(t) = &it.table {
        it.height = t.row_heights().iter().sum();
    }
}

/// `t` with `change` applied, or `None` when it is refused (a limit, a row or column it hasn't
/// got). Columns keep their width, so the table widens or narrows.
fn changed(it: &Item, change: TableEdit) -> Option<Item> {
    let t = it.table.as_ref()?;
    let mut n = it.clone();
    let col_w = it.width / t.cols as f64;
    let nt = n.table.as_mut()?;
    match change {
        TableEdit::AddRow(r) if t.rows < MAX_ROWS && r < t.rows => {
            let at = (r + 1) * t.cols;
            nt.cells.splice(at..at, vec![String::new(); t.cols]);
            nt.cell_heights.splice(at..at, vec![0.0; t.cols]);
            nt.cell_rec_ms.splice(at..at, vec![None; t.cols]);
            nt.row_min_heights.insert(r + 1, t.row_min_heights[r]);
            nt.rows += 1;
        }
        TableEdit::AddColumn(c) if t.cols < MAX_COLS && c < t.cols => {
            insert_col(&mut nt.cells, t.cols, c, String::new());
            insert_col(&mut nt.cell_heights, t.cols, c, 0.0);
            insert_col(&mut nt.cell_rec_ms, t.cols, c, None);
            nt.cols += 1;
            n.width += col_w;
        }
        TableEdit::RemoveRow(r) if t.rows > 1 && r < t.rows => {
            let keep = |i: &usize| i / t.cols != r;
            retain_index(&mut nt.cells, keep);
            retain_index(&mut nt.cell_heights, keep);
            retain_index(&mut nt.cell_rec_ms, keep);
            nt.row_min_heights.remove(r);
            nt.rows -= 1;
        }
        TableEdit::RemoveColumn(c) if t.cols > 1 && c < t.cols => {
            let keep = |i: &usize| i % t.cols != c;
            retain_index(&mut nt.cells, keep);
            retain_index(&mut nt.cell_heights, keep);
            retain_index(&mut nt.cell_rec_ms, keep);
            nt.cols -= 1;
            n.width -= col_w;
        }
        TableEdit::ResizeRow(r, h) if r < t.rows && h.is_finite() => {
            nt.row_min_heights[r] = h.clamp(MIN_ROW_HEIGHT, MAX_ROW_HEIGHT);
        }
        _ => return None,
    }
    refit(&mut n);
    Some(n)
}

/// A blank cell after column `c` in every row of a grid `cols` wide.
fn insert_col<T: Clone>(v: &mut Vec<T>, cols: usize, c: usize, blank: T) {
    *v = v
        .chunks(cols)
        .flat_map(|row| {
            let mut row = row.to_vec();
            row.insert(c + 1, blank.clone());
            row
        })
        .collect();
}

fn retain_index<T>(v: &mut Vec<T>, keep: impl Fn(&usize) -> bool) {
    let mut i = 0;
    v.retain(|_| {
        let k = keep(&i);
        i += 1;
        k
    });
}

/// The cell under a world point: `(row, col)` in the table's own (rotated) frame.
#[must_use]
pub fn cell_at(it: &Item, p: Pt) -> Option<(usize, usize)> {
    let t = it.table.as_ref()?;
    let l = geom::to_local(it, p);
    if l.x < it.x || l.y < it.y || l.x > it.x + it.width {
        return None;
    }
    let mut bottom = it.y;
    let row = t.row_heights().iter().position(|h| {
        bottom += h;
        l.y <= bottom
    })?;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // inside the table
    let col = (((l.x - it.x) / (it.width / t.cols as f64)) as usize).min(t.cols - 1);
    Some((row, col))
}

impl Scene {
    /// F21-FR10: a table from a pen drag, in whole cells, in the default style; one step.
    pub fn create_table(&mut self, from: Pt, to: Pt, made: &Made<'_>) -> Option<Ref> {
        if ![from.x, from.y, to.x, to.y].iter().all(|v| v.is_finite()) {
            return None;
        }
        let (rows, cols) = grid_for_drag((to.x - from.x).abs(), (to.y - from.y).abs());
        let mut it = Item::new("", "table", made.style.clone());
        it.rec_ms = made.rec_ms;
        it.created_at = Some(made.created_at.to_owned());
        (it.x, it.y) = (from.x.min(to.x), from.y.min(to.y));
        #[allow(clippy::cast_precision_loss)]
        let (w, h) = (cols as f64 * CELL_WIDTH, rows as f64 * MIN_ROW_HEIGHT);
        (it.width, it.height) = (w, h);
        it.table = Some(Table::empty(rows, cols));
        self.add_item(it)
    }

    /// F21-FR10: a cell's text, with its text height as measured; a row grows with its text.
    /// A cell's first text written while recording times that cell (OQ36).
    pub fn set_cell(
        &mut self,
        r: &Ref,
        row: usize,
        col: usize,
        text: &str,
        height: Option<f64>,
        rec_ms: Option<u64>,
    ) -> Option<BBox> {
        let it = self.editable_item(r)?.clone();
        let i = it.table.as_ref()?.index(row, col)?;
        if text.len() > super::item::MAX_TEXT {
            return None;
        }
        self.edit_item(&it.id, |n| {
            let Some(t) = n.table.as_mut() else { return };
            let first = t.cells[i].is_empty();
            t.cells[i] = text.to_owned();
            t.cell_heights[i] = height
                .filter(|h| h.is_finite())
                .unwrap_or(0.0)
                .clamp(0.0, MAX_ROW_HEIGHT);
            if first && !text.is_empty() && t.cell_rec_ms[i].is_none() {
                t.cell_rec_ms[i] = rec_ms;
            }
            refit(n);
        })
    }

    /// F21-FR10: one of the Table actions on table `r`; `None` when refused.
    pub fn edit_table(&mut self, r: &Ref, change: TableEdit) -> Option<BBox> {
        let it = self.editable_item(r)?.clone();
        let n = changed(&it, change)?;
        self.edit_item(&it.id, |x| *x = n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::scene::tests::with_item;
    use crate::canvas::style::Style;
    use crate::ink::InkLayer;

    fn table(rows: usize, cols: usize) -> (Scene, Ref) {
        let mut s = Scene::load(InkLayer::new(), Ok(None)).0;
        let mut it = Item::new("t", "table", Style::default());
        #[allow(clippy::cast_precision_loss)]
        let (w, h) = (cols as f64 * CELL_WIDTH, rows as f64 * MIN_ROW_HEIGHT);
        (it.width, it.height) = (w, h);
        let mut t = Table::empty(rows, cols);
        for (i, c) in t.cells.iter_mut().enumerate() {
            *c = format!("c{i}");
        }
        it.table = Some(t);
        with_item(&mut s, it);
        (s, Ref::Item("t".into()))
    }

    fn cells(s: &Scene) -> Vec<String> {
        s.item("t").unwrap().table.clone().unwrap().cells
    }

    /// sn-canvas TableElementsTest: a drag grows in whole cells, within 12 × 8.
    #[test]
    fn a_drag_makes_whole_cells() {
        assert_eq!(grid_for_drag(330.0, 100.0), (2, 2));
        assert_eq!(grid_for_drag(0.0, 0.0), (1, 1));
        assert_eq!(grid_for_drag(1e6, 1e6), (12, 8));
        let mut s = Scene::load(InkLayer::new(), Ok(None)).0;
        let made = Made {
            style: Style::default(),
            rec_ms: Some(4),
            created_at: "t",
        };
        let r = s
            .create_table(Pt::new(500.0, 200.0), Pt::new(0.0, 0.0), &made)
            .unwrap();
        let Ref::Item(id) = &r else { panic!() };
        let it = s.item(id).unwrap();
        assert_eq!((it.x, it.width, it.height), (0.0, 480.0, 192.0));
        assert_eq!(it.table.as_ref().unwrap().rows, 4);
        assert_eq!(
            s.create_table(Pt::new(f64::NAN, 0.0), Pt::new(1.0, 1.0), &made),
            None
        );
    }

    /// F21-AC10: a 12 × 8 table refuses Add row and Add column; Remove this row on (2, 1) of a
    /// 3 × 3 removes row 2 and keeps the rest; a new row is as tall as the row above.
    #[test]
    fn rows_and_columns_go_where_the_target_is() {
        let (mut big, b) = table(12, 8);
        assert_eq!(big.edit_table(&b, TableEdit::AddRow(3)), None);
        assert_eq!(big.edit_table(&b, TableEdit::AddColumn(3)), None);
        let (mut s, t) = table(3, 3);
        assert!(s.edit_table(&t, TableEdit::RemoveRow(2)).is_some());
        assert_eq!(cells(&s), ["c0", "c1", "c2", "c3", "c4", "c5"]);
        s.undo();
        assert!(s.edit_table(&t, TableEdit::RemoveRow(1)).is_some());
        assert_eq!(
            cells(&s),
            ["c0", "c1", "c2", "c6", "c7", "c8"],
            "row 1, not the last"
        );
        s.undo();
        assert!(s.edit_table(&t, TableEdit::ResizeRow(0, 90.0)).is_some());
        assert!(s.edit_table(&t, TableEdit::AddRow(0)).is_some());
        let it = s.item("t").unwrap().clone();
        let tb = it.table.unwrap();
        assert_eq!(
            tb.row_min_heights,
            vec![90.0, 90.0, 48.0, 48.0],
            "as tall as the row above"
        );
        assert_eq!(&tb.cells[3..6], ["", "", ""]);
        assert_eq!(it.height, 276.0);
        assert!(s.edit_table(&t, TableEdit::AddColumn(0)).is_some());
        let tb = s.item("t").unwrap().table.clone().unwrap();
        assert_eq!(
            (tb.cols, &tb.cells[0..4]),
            (
                4,
                &[
                    "c0".to_owned(),
                    String::new(),
                    "c1".to_owned(),
                    "c2".to_owned()
                ][..]
            )
        );
        assert_eq!(s.item("t").unwrap().width, 640.0);
        assert!(s.edit_table(&t, TableEdit::RemoveColumn(2)).is_some());
        let tb = s.item("t").unwrap().table.clone().unwrap();
        assert_eq!(&tb.cells[0..3], ["c0", "", "c2"]);
        assert_eq!(tb.cell_heights.len(), tb.cells.len());
        assert_eq!(tb.cell_rec_ms.len(), tb.cells.len());
        let (mut one, o) = table(1, 1);
        assert_eq!(
            one.edit_table(&o, TableEdit::RemoveRow(0)),
            None,
            "disabled at one row"
        );
        assert_eq!(one.edit_table(&o, TableEdit::RemoveColumn(0)), None);
        assert_eq!(one.edit_table(&o, TableEdit::AddRow(5)), None);
        assert_eq!(one.edit_table(&o, TableEdit::ResizeRow(0, f64::NAN)), None);
        assert_eq!(
            one.edit_table(&Ref::Item("zz".into()), TableEdit::AddRow(0)),
            None
        );
    }

    #[test]
    fn cells_take_text_heights_and_their_first_time() {
        let (mut s, t) = table(2, 2);
        assert!(s.set_cell(&t, 1, 0, "", None, Some(1)).is_some());
        assert!(
            s.set_cell(&t, 0, 1, "Owner", Some(130.0), Some(761_000))
                .is_some()
        );
        let it = s.item("t").unwrap().clone();
        let tb = it.table.unwrap();
        assert_eq!(
            tb.row_heights(),
            vec![130.0, 48.0],
            "a row grows with its text"
        );
        assert_eq!(it.height, 178.0);
        assert_eq!(tb.cell_rec_ms[1], None, "it had text already");
        s.set_cell(&t, 1, 0, "first", Some(10.0), Some(5));
        assert_eq!(
            s.item("t").unwrap().table.clone().unwrap().cell_rec_ms[2],
            Some(5)
        );
        assert_eq!(s.set_cell(&t, 2, 0, "x", None, None), None, "no such cell");
        assert_eq!(s.set_cell(&t, 0, 0, &"x".repeat(70_000), None, None), None);
        let it = s.item("t").unwrap();
        assert_eq!(cell_at(it, Pt::new(170.0, 140.0)), Some((1, 1)));
        assert_eq!(cell_at(it, Pt::new(10.0, 10.0)), Some((0, 0)));
        assert_eq!(cell_at(it, Pt::new(-10.0, 10.0)), None);
        assert_eq!(cell_at(it, Pt::new(10.0, 9999.0)), None);
    }
}
