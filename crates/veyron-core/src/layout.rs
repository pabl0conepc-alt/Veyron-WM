//! The Veyron scrollable tiling engine.
//!
//! A workspace is an unbounded horizontal strip of **columns**. Each column
//! holds one or more windows stacked vertically, so a workspace can express both
//! the "endless row of full-height columns" model and denser stacked layouts
//! without a second layout mode.
//!
//! Invariants maintained by every mutating method:
//!
//! 1. No column is ever empty (an empty column is dropped immediately).
//! 2. `focus`, when present, always addresses an existing window.
//! 3. `scroll` is always inside `0..=max_scroll()`.
//! 4. The focused column is always fully inside the viewport when it fits.
//!
//! The engine performs no I/O, holds no protocol types and can be exercised
//! without a compositor, which is what makes the management model testable.

use serde::{Deserialize, Serialize};

use crate::{
    action::Direction,
    geometry::{Rect, Size},
    tiling::LayoutKind,
    window::WindowId,
};

/// Tiling geometry and policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LayoutConfig {
    /// Gap between columns and between stacked windows, in logical pixels.
    pub gap: i32,
    /// Margin kept between the tiled area and the output edges.
    pub outer_margin: i32,
    /// Width of a new column as a fraction of the viewport width.
    pub default_column_ratio: f64,
    /// Smallest width a column can be resized to, as a viewport fraction.
    pub min_column_ratio: f64,
    /// Largest width a column can be resized to, as a viewport fraction.
    pub max_column_ratio: f64,
    /// Tiling layout selected for new workspaces. `Auto` enables dynamic
    /// tiling: the layout is resolved from the number of tiled windows.
    pub mode: LayoutKind,
    /// Remove the gap for a lone window and tighten it on dense layouts.
    pub smart_gaps: bool,
    /// Share of the viewport given to the master window in `MasterStack`.
    pub master_ratio: f64,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            gap: 8,
            outer_margin: 12,
            default_column_ratio: 0.5,
            min_column_ratio: 0.2,
            max_column_ratio: 1.0,
            mode: LayoutKind::Scroll,
            smart_gaps: false,
            master_ratio: 0.6,
        }
    }
}

impl LayoutConfig {
    /// Configuration errors are reported as strings so the caller decides how
    /// to present them (log, IPC error, exit message).
    pub fn validate(&self) -> Result<(), String> {
        if self.gap < 0 {
            return Err(format!("layout.gap must be >= 0 (got {})", self.gap));
        }
        if self.outer_margin < 0 {
            return Err(format!(
                "layout.outer_margin must be >= 0 (got {})",
                self.outer_margin
            ));
        }
        if !(0.0..=1.0).contains(&self.default_column_ratio) || self.default_column_ratio <= 0.0 {
            return Err(format!(
                "layout.default_column_ratio must be in (0, 1] (got {})",
                self.default_column_ratio
            ));
        }
        if !(0.0..=1.0).contains(&self.min_column_ratio) || self.min_column_ratio <= 0.0 {
            return Err(format!(
                "layout.min_column_ratio must be in (0, 1] (got {})",
                self.min_column_ratio
            ));
        }
        if !(0.0..=1.0).contains(&self.max_column_ratio) || self.max_column_ratio <= 0.0 {
            return Err(format!(
                "layout.max_column_ratio must be in (0, 1] (got {})",
                self.max_column_ratio
            ));
        }
        if self.min_column_ratio > self.default_column_ratio {
            return Err(format!(
                "layout.min_column_ratio ({}) must be <= layout.default_column_ratio ({})",
                self.min_column_ratio, self.default_column_ratio
            ));
        }
        if self.default_column_ratio > self.max_column_ratio {
            return Err(format!(
                "layout.default_column_ratio ({}) must be <= layout.max_column_ratio ({})",
                self.default_column_ratio, self.max_column_ratio
            ));
        }
        if !(0.1..=0.9).contains(&self.master_ratio) {
            return Err(format!(
                "layout.master_ratio must be in [0.1, 0.9] (got {})",
                self.master_ratio
            ));
        }
        Ok(())
    }
}

/// A column's slot address inside a workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub column: usize,
    pub row: usize,
}

/// One column of the strip.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    windows: Vec<WindowId>,
    ratio: f64,
}

impl Column {
    pub fn windows(&self) -> &[WindowId] {
        &self.windows
    }

    pub fn len(&self) -> usize {
        self.windows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    pub fn ratio(&self) -> f64 {
        self.ratio
    }
}

/// A window together with the geometry the layout assigned to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub window: WindowId,
    pub rect: Rect,
}

/// The scrollable tiling engine of a single workspace.
#[derive(Debug, Clone)]
pub struct ScrollLayout {
    config: LayoutConfig,
    columns: Vec<Column>,
    focus: Option<Position>,
    scroll: f64,
    viewport: Rect,
}

impl ScrollLayout {
    pub fn new(config: LayoutConfig, viewport: Rect) -> Self {
        Self {
            config,
            columns: Vec::new(),
            focus: None,
            scroll: 0.0,
            viewport,
        }
    }

    pub fn config(&self) -> &LayoutConfig {
        &self.config
    }

    pub fn set_config(&mut self, config: LayoutConfig) {
        self.config = config;
        let (min, max) = self.ratio_bounds();
        for column in &mut self.columns {
            column.ratio = column.ratio.clamp(min, max);
        }
        self.ensure_focus_visible();
    }

    pub fn viewport(&self) -> Rect {
        self.viewport
    }

    /// Update the usable area (output size minus outer margins).
    pub fn set_viewport(&mut self, viewport: Rect) {
        self.viewport = viewport;
        self.ensure_focus_visible();
    }

    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    pub fn window_count(&self) -> usize {
        self.columns.iter().map(Column::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    pub fn focus(&self) -> Option<Position> {
        self.focus
    }

    pub fn focused_window(&self) -> Option<WindowId> {
        let focus = self.focus?;
        self.columns
            .get(focus.column)
            .and_then(|column| column.windows.get(focus.row))
            .copied()
    }

    pub fn scroll(&self) -> f64 {
        self.scroll
    }

    /// Windows in strip order, left to right and top to bottom.
    pub fn windows(&self) -> impl Iterator<Item = WindowId> + '_ {
        self.columns
            .iter()
            .flat_map(|column| column.windows.iter().copied())
    }

    /// Every window in strip order, collected.
    pub fn order(&self) -> Vec<WindowId> {
        self.windows().collect()
    }

    /// Move focus to the previous (`delta < 0`) or next (`delta > 0`) window in
    /// strip order. Used by layouts where `left/right` means "previous/next"
    /// rather than "neighbouring column".
    pub fn focus_order_neighbor(&mut self, delta: isize) -> bool {
        let order = self.order();
        let Some(current) = self.focused_window() else {
            return false;
        };
        let Some(index) = order.iter().position(|window| *window == current) else {
            return false;
        };
        let Some(target) = index.checked_add_signed(delta) else {
            return false;
        };
        let Some(target) = order.get(target).copied() else {
            return false;
        };
        self.focus_window(target)
    }

    /// Swap the focused window with its predecessor/successor in strip order.
    ///
    /// The strip is rebuilt with one window per column, preserving every
    /// window's current ratio, so the other layouts keep a single, predictable
    /// order source of truth.
    pub fn move_order_neighbor(&mut self, delta: isize) -> bool {
        let mut order = self.order();
        let Some(current) = self.focused_window() else {
            return false;
        };
        let Some(index) = order.iter().position(|window| *window == current) else {
            return false;
        };
        let Some(target) = index.checked_add_signed(delta) else {
            return false;
        };
        if target >= order.len() {
            return false;
        }
        order.swap(index, target);

        let ratios: Vec<(WindowId, f64)> = self
            .columns
            .iter()
            .flat_map(|column| {
                column
                    .windows
                    .iter()
                    .map(move |window| (*window, column.ratio))
            })
            .collect();
        let (min, max) = self.ratio_bounds();
        let default_ratio = self.config.default_column_ratio.clamp(min, max);
        self.columns = order
            .iter()
            .map(|window| Column {
                windows: vec![*window],
                ratio: ratios
                    .iter()
                    .find(|(candidate, _)| candidate == window)
                    .map(|(_, ratio)| ratio.clamp(min, max))
                    .unwrap_or(default_ratio),
            })
            .collect();
        self.focus = Some(Position {
            column: target,
            row: 0,
        });
        self.ensure_focus_visible();
        true
    }

    pub fn contains(&self, window: WindowId) -> bool {
        self.position_of(window).is_some()
    }

    pub fn position_of(&self, window: WindowId) -> Option<Position> {
        self.columns.iter().enumerate().find_map(|(column, col)| {
            col.windows
                .iter()
                .position(|candidate| *candidate == window)
                .map(|row| Position { column, row })
        })
    }

    /// Insert a window as a new column immediately right of the focused column.
    ///
    /// New windows always open as full-height columns: stacking is an explicit
    /// user action ([`ScrollLayout::move_window`]), which keeps insertion
    /// predictable and avoids silently rescaling existing windows.
    pub fn insert_window(&mut self, window: WindowId) {
        if self.contains(window) {
            return;
        }
        let (min, max) = self.ratio_bounds();
        let ratio = self.config.default_column_ratio.clamp(min, max);
        let index = match self.focus {
            Some(focus) => (focus.column + 1).min(self.columns.len()),
            None => self.columns.len(),
        };
        self.columns.insert(
            index,
            Column {
                windows: vec![window],
                ratio,
            },
        );
        self.focus = Some(Position {
            column: index,
            row: 0,
        });
        self.ensure_focus_visible();
    }

    /// Remove a window, dropping its column when it becomes empty.
    pub fn remove_window(&mut self, window: WindowId) -> bool {
        let Some(position) = self.position_of(window) else {
            return false;
        };

        self.columns[position.column].windows.remove(position.row);
        let column_dropped = self.columns[position.column].windows.is_empty();
        if column_dropped {
            self.columns.remove(position.column);
        }

        let Some(mut focus) = self.focus else {
            return true;
        };

        if focus.column > position.column {
            focus.column -= 1;
        } else if focus.column == position.column && column_dropped {
            focus.column = position.column.min(self.columns.len().saturating_sub(1));
        }

        if self.columns.is_empty() {
            self.focus = None;
            self.scroll = 0.0;
            return true;
        }

        let column = &self.columns[focus.column];
        focus.row = focus.row.min(column.windows.len() - 1);
        self.focus = Some(focus);
        self.ensure_focus_visible();
        true
    }

    /// Focus a specific window. Returns whether the focus actually moved.
    pub fn focus_window(&mut self, window: WindowId) -> bool {
        let Some(position) = self.position_of(window) else {
            return false;
        };
        let changed = self.focus != Some(position);
        self.focus = Some(position);
        self.ensure_focus_visible();
        changed
    }

    /// Move focus to a neighbouring column or row.
    pub fn focus_direction(&mut self, direction: Direction) -> bool {
        let Some(focus) = self.focus else {
            return false;
        };
        let target = match direction {
            Direction::Left => {
                if focus.column == 0 {
                    return false;
                }
                Position {
                    column: focus.column - 1,
                    row: focus.row,
                }
            }
            Direction::Right => {
                if focus.column + 1 >= self.columns.len() {
                    return false;
                }
                Position {
                    column: focus.column + 1,
                    row: focus.row,
                }
            }
            Direction::Up => {
                if focus.row == 0 {
                    return false;
                }
                Position {
                    column: focus.column,
                    row: focus.row - 1,
                }
            }
            Direction::Down => {
                if focus.row + 1 >= self.columns[focus.column].windows.len() {
                    return false;
                }
                Position {
                    column: focus.column,
                    row: focus.row + 1,
                }
            }
        };

        let target = Position {
            column: target.column,
            row: target
                .row
                .min(self.columns[target.column].windows.len() - 1),
        };
        if target == focus {
            return false;
        }
        self.focus = Some(target);
        self.ensure_focus_visible();
        true
    }

    /// Move the focused window into the neighbouring column, or swap it with the
    /// neighbouring row inside its column.
    pub fn move_window(&mut self, direction: Direction) -> bool {
        let Some(focus) = self.focus else {
            return false;
        };

        match direction {
            Direction::Left | Direction::Right => {
                let target = match direction {
                    Direction::Left if focus.column > 0 => focus.column - 1,
                    Direction::Right if focus.column + 1 < self.columns.len() => focus.column + 1,
                    _ => return false,
                };
                let window = self.columns[focus.column].windows.remove(focus.row);
                let source = focus.column;
                let source_dropped = self.columns[source].windows.is_empty();
                if source_dropped {
                    self.columns.remove(source);
                }
                let target = if source_dropped && target > source {
                    target - 1
                } else {
                    target
                };
                let row = self.columns[target].windows.len();
                self.columns[target].windows.push(window);
                self.focus = Some(Position {
                    column: target,
                    row,
                });
            }
            Direction::Up | Direction::Down => {
                let neighbor = match direction {
                    Direction::Up if focus.row > 0 => focus.row - 1,
                    Direction::Down if focus.row + 1 < self.columns[focus.column].windows.len() => {
                        focus.row + 1
                    }
                    _ => return false,
                };
                self.columns[focus.column].windows.swap(focus.row, neighbor);
                self.focus = Some(Position {
                    column: focus.column,
                    row: neighbor,
                });
            }
        }

        self.ensure_focus_visible();
        true
    }

    /// Resize the focused column by `delta` percent of the viewport width.
    pub fn resize_focused(&mut self, delta: i32) -> bool {
        let Some(focus) = self.focus else {
            return false;
        };
        let (min, max) = self.ratio_bounds();
        let current = self.columns[focus.column].ratio;
        let updated = (current + delta as f64 / 100.0).clamp(min, max);
        if (updated - current).abs() < f64::EPSILON {
            return false;
        }
        self.columns[focus.column].ratio = updated;
        self.ensure_focus_visible();
        true
    }

    /// Shift the viewport by `delta` pixels along the strip.
    pub fn scroll_by(&mut self, delta: f64) -> bool {
        let before = self.scroll;
        self.scroll += delta;
        self.clamp_scroll();
        (self.scroll - before).abs() > f64::EPSILON
    }

    /// Center the focused column in the viewport.
    pub fn center_focus(&mut self) -> bool {
        let Some(focus) = self.focus else {
            return false;
        };
        let before = self.scroll;
        let start = self.column_offset(focus.column) as f64;
        let width = self.column_outer_width(focus.column) - self.gap();
        self.scroll = start + width as f64 / 2.0 - self.viewport.w as f64 / 2.0;
        self.clamp_scroll();
        (self.scroll - before).abs() > f64::EPSILON
    }

    /// Geometry of every window in strip order.
    pub fn placements(&self) -> Vec<Placement> {
        self.placements_with_gap(self.gap())
    }

    /// Geometry with an explicit gap, used by smart gaps.
    pub fn placements_with_gap(&self, gap: i32) -> Vec<Placement> {
        if self.viewport.is_empty() {
            return Vec::new();
        }

        let gap = gap.max(0);
        let mut placements = Vec::with_capacity(self.window_count());
        let mut x = self.viewport.x as f64 - self.scroll;

        for (index, column) in self.columns.iter().enumerate() {
            let outer = self.column_outer_width(index);
            let width = (outer - gap).max(1);
            let rows = column.windows.len() as i32;

            if rows > 0 {
                let available = (self.viewport.h - gap * (rows - 1)).max(rows);
                let row_height = available / rows;
                let mut y = self.viewport.y;
                for (row, window) in column.windows.iter().enumerate() {
                    let height = if row as i32 == rows - 1 {
                        available - row_height * (rows - 1)
                    } else {
                        row_height
                    };
                    placements.push(Placement {
                        window: *window,
                        rect: Rect::new(x.round() as i32, y, width, height),
                    });
                    y += height + gap;
                }
            }

            x += outer as f64;
        }

        placements
    }

    pub fn rect_of(&self, window: WindowId) -> Option<Rect> {
        self.placements()
            .into_iter()
            .find(|placement| placement.window == window)
            .map(|placement| placement.rect)
    }

    /// Keep the focused column inside the viewport, scrolling only as much as
    /// necessary. Columns wider than the viewport are aligned to their left edge.
    pub fn ensure_focus_visible(&mut self) {
        let Some(focus) = self.focus else {
            self.scroll = 0.0;
            return;
        };
        if self.viewport.w <= 0 {
            return;
        }

        let start = self.column_offset(focus.column) as f64;
        let end = start + (self.column_outer_width(focus.column) - self.gap()).max(1) as f64;
        let view = self.viewport.w as f64;

        if end - self.scroll > view {
            self.scroll = end - view;
        }
        if start - self.scroll < 0.0 {
            self.scroll = start;
        }
        self.clamp_scroll();
    }

    /// Largest possible scroll offset for the current strip and viewport.
    pub fn max_scroll(&self) -> f64 {
        if self.columns.is_empty() || self.viewport.w <= 0 {
            return 0.0;
        }
        let content = self.content_width();
        (content - self.viewport.w).max(0) as f64
    }

    fn gap(&self) -> i32 {
        self.config.gap.max(0)
    }

    fn ratio_bounds(&self) -> (f64, f64) {
        let min = self.config.min_column_ratio.clamp(0.01, 1.0);
        let max = self.config.max_column_ratio.clamp(min, 1.0);
        (min, max)
    }

    /// Outer slot width of a column: the column plus the gap that separates it
    /// from the next one. Measured in whole logical pixels so that the strip can
    /// be scrolled without drift.
    fn column_outer_width(&self, index: usize) -> i32 {
        let ratio = self.columns[index].ratio;
        (ratio * self.viewport.w as f64).round().max(1.0) as i32
    }

    fn column_offset(&self, index: usize) -> i32 {
        (0..index)
            .map(|previous| self.column_outer_width(previous))
            .sum()
    }

    fn content_width(&self) -> i32 {
        let total: i32 = (0..self.columns.len())
            .map(|index| self.column_outer_width(index))
            .sum();
        (total - self.gap()).max(1)
    }

    fn clamp_scroll(&mut self) {
        let max = self.max_scroll();
        if self.scroll < 0.0 {
            self.scroll = 0.0;
        } else if self.scroll > max {
            self.scroll = max;
        }
    }
}

/// Convenience: the viewport a layout gets for an output rectangle.
pub fn viewport_for(output: Rect, config: &LayoutConfig) -> Rect {
    let inner = output.inset(config.outer_margin);
    if inner.is_empty() {
        // Never hand an empty viewport to the layout: fall back to the full
        // output so a tiny output still produces usable geometry.
        Rect::new(output.x, output.y, output.w.max(1), output.h.max(1))
    } else {
        inner
    }
}

/// Default size used for a new floating window.
pub fn default_floating_size(viewport: Rect) -> Size {
    Size::new((viewport.w * 3 / 5).max(1), (viewport.h * 3 / 5).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(width: i32, height: i32) -> ScrollLayout {
        ScrollLayout::new(LayoutConfig::default(), Rect::new(0, 0, width, height))
    }

    fn window(id: u64) -> WindowId {
        WindowId(id)
    }

    #[test]
    fn new_windows_open_as_columns_to_the_right() {
        let mut layout = layout(1000, 800);
        layout.insert_window(window(1));
        layout.insert_window(window(2));
        layout.insert_window(window(3));

        assert_eq!(layout.column_count(), 3);
        assert_eq!(layout.focused_window(), Some(window(3)));
        assert_eq!(
            layout.position_of(window(3)),
            Some(Position { column: 2, row: 0 })
        );

        // The viewport follows the focus: the third column is fully visible
        // while the first two are scrolled off to the left.
        assert_eq!(layout.scroll(), 492.0);
        let placements = layout.placements();
        assert_eq!(placements.len(), 3);
        // 50% columns with an 8px gap: 500px slots, 492px content.
        assert_eq!(placements[0].rect.x, -492);
        assert_eq!(placements[1].rect.x, 8);
        assert_eq!(placements[2].rect.x, 508);
        assert_eq!(placements[2].rect.right(), 1000);
        assert_eq!(placements[0].rect.w, 492);
        assert_eq!(placements[0].rect.h, 800);
    }

    #[test]
    fn focus_scrolls_the_strip() {
        let mut layout = layout(1000, 800);
        for id in 1..=3 {
            layout.insert_window(window(id));
        }

        // Three 500px slots: 1500-8 = 1492px of content in a 1000px viewport.
        assert_eq!(layout.max_scroll(), 492.0);
        assert_eq!(layout.scroll(), 492.0);

        assert!(layout.focus_direction(Direction::Left));
        assert_eq!(layout.focused_window(), Some(window(2)));
        let rect = layout
            .rect_of(window(2))
            .expect("focused window has geometry");
        assert!(
            rect.x >= 0 && rect.right() <= 1000,
            "focused column is visible"
        );

        assert!(layout.focus_direction(Direction::Right));
        assert_eq!(layout.focused_window(), Some(window(3)));
        assert!(!layout.focus_direction(Direction::Right));
        assert_eq!(
            layout.scroll(),
            492.0,
            "focus scrolls the strip back to the end"
        );
    }

    #[test]
    fn moving_right_stacks_windows_in_one_column() {
        let mut layout = layout(1000, 600);
        layout.insert_window(window(1));
        layout.insert_window(window(2));

        assert!(layout.move_window(Direction::Left));
        assert_eq!(layout.column_count(), 1);
        assert_eq!(layout.columns()[0].windows(), &[window(1), window(2)]);
        assert_eq!(layout.focused_window(), Some(window(2)));

        let placements = layout.placements();
        assert_eq!(placements.len(), 2);
        // 600px tall viewport, 8px gap, split evenly.
        assert_eq!(placements[0].rect, Rect::new(0, 0, 492, 296));
        assert_eq!(placements[1].rect, Rect::new(0, 304, 492, 296));
    }

    #[test]
    fn moving_left_into_the_only_column_keeps_the_strip_consistent() {
        let mut layout = layout(800, 600);
        layout.insert_window(window(1));
        layout.insert_window(window(2));

        // Window 1's column is the leftmost one: nothing to its left.
        assert!(layout.focus_direction(Direction::Left));
        assert!(!layout.move_window(Direction::Left));

        assert!(layout.move_window(Direction::Right));
        assert_eq!(layout.column_count(), 1);
        assert_eq!(layout.columns()[0].windows(), &[window(2), window(1)]);
        assert_eq!(layout.focused_window(), Some(window(1)));
    }

    #[test]
    fn removing_the_last_column_falls_back_to_the_previous_one() {
        let mut layout = layout(1000, 600);
        layout.insert_window(window(1));
        layout.insert_window(window(2));
        layout.insert_window(window(3));

        assert!(layout.remove_window(window(3)));
        assert_eq!(layout.column_count(), 2);
        assert_eq!(layout.focused_window(), Some(window(2)));
        assert_eq!(layout.focus(), Some(Position { column: 1, row: 0 }));

        assert!(layout.remove_window(window(2)));
        assert_eq!(layout.focused_window(), Some(window(1)));

        assert!(layout.remove_window(window(1)));
        assert_eq!(layout.focus(), None);
        assert_eq!(layout.scroll(), 0.0);
        assert!(layout.placements().is_empty());
    }

    #[test]
    fn removing_an_unfocused_window_keeps_the_focus_target_valid() {
        let mut layout = layout(1200, 700);
        for id in 1..=4 {
            layout.insert_window(window(id));
        }
        assert_eq!(layout.focused_window(), Some(window(4)));

        assert!(layout.remove_window(window(2)));
        assert_eq!(layout.focused_window(), Some(window(4)));
        assert_eq!(
            layout.position_of(window(4)),
            Some(Position { column: 2, row: 0 })
        );
    }

    #[test]
    fn column_resize_is_clamped_to_the_configured_bounds() {
        let mut layout = layout(1000, 600);
        layout.insert_window(window(1));

        assert!(layout.resize_focused(60));
        assert_eq!(layout.columns()[0].ratio(), 1.0);
        assert!(!layout.resize_focused(10));
        assert_eq!(layout.columns()[0].ratio(), 1.0);

        assert!(layout.resize_focused(-90));
        assert_eq!(layout.columns()[0].ratio(), 0.2);
        assert_eq!(layout.placements()[0].rect.w, 192);
        assert_eq!(layout.placements()[0].rect.h, 600);
    }

    #[test]
    fn scroll_offset_is_clamped_and_center_respects_bounds() {
        let mut layout = layout(500, 400);
        for id in 1..=4 {
            layout.insert_window(window(id));
        }
        // Four 250px slots, 992px of content in a 500px viewport.
        assert_eq!(layout.max_scroll(), 492.0);

        layout.scroll_by(10_000.0);
        assert_eq!(layout.scroll(), 492.0);
        layout.scroll_by(-10_000.0);
        assert_eq!(layout.scroll(), 0.0);
    }

    #[test]
    fn viewport_resize_reclamps_the_scroll() {
        let mut layout = layout(1000, 600);
        for id in 1..=4 {
            layout.insert_window(window(id));
        }
        assert_eq!(layout.scroll(), 992.0);

        layout.set_viewport(Rect::new(0, 0, 2000, 900));
        // Columns grow with the viewport, so the strip is still wider than the
        // viewport and the scroll offset is re-clamped, never left dangling.
        assert!(layout.scroll() <= layout.max_scroll());
        let focused = layout.focused_window().expect("still focused");
        let rect = layout.rect_of(focused).expect("focused geometry");
        assert!(rect.x >= 0 && rect.right() <= 2000);

        layout.set_viewport(Rect::new(0, 0, 800, 600));
        let placements = layout.placements();
        assert_eq!(placements.len(), 4);
        for placement in placements {
            assert!(placement.rect.w > 0);
            assert!(placement.rect.h > 0);
        }
    }

    #[test]
    fn stacked_rows_always_fill_the_viewport_height() {
        let mut layout = layout(600, 500);
        for id in 1..=4 {
            layout.insert_window(window(id));
            if id > 1 {
                assert!(layout.move_window(Direction::Left));
            }
        }
        assert_eq!(layout.column_count(), 1);
        assert_eq!(layout.columns()[0].len(), 4);

        let placements = layout.placements();
        let top = placements.first().map(|p| p.rect.y);
        let bottom = placements.last().map(|p| p.rect.bottom());
        assert_eq!(top, Some(0));
        assert_eq!(bottom, Some(500));
    }

    #[test]
    fn inserting_an_unknown_window_is_idempotent() {
        let mut layout = layout(800, 600);
        layout.insert_window(window(1));
        layout.insert_window(window(1));
        assert_eq!(layout.window_count(), 1);
    }
}
