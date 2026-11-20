//! Workspace overview layout — the policy behind an exposé-style grid.
//!
//! The compositor draws the overview; this module decides *where* every
//! workspace of an output appears, which workspace a click lands on and how a
//! large number of workspaces is paged. Keeping that here means the grid can be
//! verified without a renderer, and the compositor only has to draw rectangles.
//!
//! Workspaces are ordered by their sparse coordinate (`y`, then `x`), so the
//! overview mirrors the spatial desktop instead of creation order.

use serde::{Deserialize, Serialize};

use crate::{
    geometry::{Point, Rect},
    wm::{Wm, WorkspacePosition},
};

/// Hard cap on the workspaces shown in one overview. Anything beyond it is
/// paged, which keeps a runaway workspace count from producing huge grids.
pub const MAX_ENTRIES: usize = 64;

/// Geometry of the overview grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OverviewConfig {
    /// Grid columns. Clamped to `1..=8` when the grid is built.
    pub columns: usize,
    /// Space between cells, in logical pixels.
    pub gap: i32,
    /// Space kept around the grid inside the viewport.
    pub margin: i32,
}

impl Default for OverviewConfig {
    fn default() -> Self {
        Self {
            columns: 4,
            gap: 12,
            margin: 24,
        }
    }
}

impl OverviewConfig {
    fn columns(&self) -> usize {
        self.columns.clamp(1, 8)
    }

    fn gap(&self) -> i32 {
        self.gap.max(0)
    }

    fn margin(&self) -> i32 {
        self.margin.max(0)
    }

    /// Cells on one page: a square of `columns` rows.
    pub fn page_size(&self) -> usize {
        let columns = self.columns();
        columns * columns
    }
}

/// One workspace inside the overview grid.
#[derive(Debug, Clone, PartialEq)]
pub struct OverviewEntry {
    pub output_index: usize,
    /// Index of the workspace in its output.
    pub workspace_index: usize,
    pub name: String,
    pub position: WorkspacePosition,
    pub rect: Rect,
    /// Tiled plus floating windows on the workspace.
    pub windows: usize,
    /// Whether this is the workspace its output is showing.
    pub active: bool,
    /// Whether the workspace's output is the one that has focus.
    pub focused: bool,
}

/// A page of the overview for one output.
#[derive(Debug, Clone, PartialEq)]
pub struct Overview {
    /// Full output rectangle the overview covers.
    pub output: Rect,
    /// Available area inside the configured margin.
    pub viewport: Rect,
    pub output_index: usize,
    /// Zero-based page, and how many pages exist.
    pub page: usize,
    pub pages: usize,
    pub columns: usize,
    pub rows: usize,
    pub entries: Vec<OverviewEntry>,
}

impl Overview {
    /// Build the overview of an output, showing the page that contains its
    /// active workspace.
    pub fn build(wm: &Wm, output_index: usize, config: &OverviewConfig) -> Option<Overview> {
        let page = {
            let output = wm.outputs().get(output_index)?;
            let page_size = config.page_size().max(1);
            let active = output.active_workspace_index();
            let positions = spatial_order(wm, output_index);
            positions
                .iter()
                .position(|(_, index)| *index == active)
                .map(|slot| slot / page_size)
                .unwrap_or(0)
        };
        Overview::build_page(wm, output_index, config, page)
    }

    /// Build a specific page of an output's overview. The page index is
    /// clamped into range, so a stale index after closing workspaces is safe.
    pub fn build_page(
        wm: &Wm,
        output_index: usize,
        config: &OverviewConfig,
        page: usize,
    ) -> Option<Overview> {
        let output = wm.outputs().get(output_index)?;
        let active = output.active_workspace_index();
        let focused = wm.active_output_index() == output_index;

        let positions = spatial_order(wm, output_index);
        let columns = config.columns();
        let page_size = config.page_size().max(1);
        let pages = positions.len().div_ceil(page_size).max(1);
        let page = page.min(pages - 1);
        let viewport = viewport_for(output.rect(), config);
        let start = page * page_size;
        let slice = &positions[start..(start + page_size).min(positions.len())];
        let rows = slice.len().div_ceil(columns).max(1);
        let entries = slice
            .iter()
            .enumerate()
            .map(|(slot, (position, index))| {
                let workspace = &output.workspaces()[*index];
                let windows =
                    workspace.layout().window_count() + workspace.floating_windows().len();
                OverviewEntry {
                    output_index,
                    workspace_index: *index,
                    name: workspace.name().to_string(),
                    position: *position,
                    rect: cell_rect(viewport, config, columns, rows, slot),
                    windows,
                    active: *index == active,
                    focused,
                }
            })
            .collect();

        Some(Overview {
            output: output.rect(),
            viewport,
            output_index,
            page,
            pages,
            columns,
            rows,
            entries,
        })
    }

    /// The entry under a point, if any.
    pub fn hit(&self, point: Point) -> Option<&OverviewEntry> {
        self.entries.iter().find(|entry| entry.rect.contains(point))
    }

    /// The entry for a workspace of this output.
    pub fn entry(&self, workspace_index: usize) -> Option<&OverviewEntry> {
        self.entries
            .iter()
            .find(|entry| entry.workspace_index == workspace_index)
    }

    /// Next page index, wrapping around; `None` with a single page.
    pub fn next_page(&self) -> Option<usize> {
        (self.pages > 1).then_some((self.page + 1) % self.pages)
    }

    pub fn previous_page(&self) -> Option<usize> {
        (self.pages > 1).then_some((self.page + self.pages - 1) % self.pages)
    }
}

impl Wm {
    /// Overview of the active output with the given grid configuration.
    pub fn overview(&self, config: &OverviewConfig) -> Overview {
        Overview::build(self, self.active_output_index(), config)
            .expect("the active output always exists")
    }
}

/// Workspaces of an output sorted by sparse coordinate, `y` before `x`, so the
/// grid mirrors the desktop rather than creation order.
fn spatial_order(wm: &Wm, output_index: usize) -> Vec<(WorkspacePosition, usize)> {
    let Some(output) = wm.outputs().get(output_index) else {
        return Vec::new();
    };
    let mut positions: Vec<(WorkspacePosition, usize)> = output
        .workspaces()
        .iter()
        .enumerate()
        .map(|(index, workspace)| (workspace.position(), index))
        .collect();
    positions.sort_by_key(|(position, index)| (position.y, position.x, *index));
    positions
}

fn viewport_for(output: Rect, config: &OverviewConfig) -> Rect {
    let inner = output.inset(config.margin());
    if inner.is_empty() {
        Rect::new(output.x, output.y, output.w.max(1), output.h.max(1))
    } else {
        inner
    }
}

fn cell_rect(
    viewport: Rect,
    config: &OverviewConfig,
    columns: usize,
    rows: usize,
    slot: usize,
) -> Rect {
    let columns = columns.max(1);
    let rows = rows.max(1);
    let gap = config.gap();
    // Distribute the leftover pixels to the last cells instead of dropping them,
    // so the grid always fills the viewport exactly.
    let usable_w = (viewport.w - gap * (columns as i32 - 1)).max(columns as i32);
    let usable_h = (viewport.h - gap * (rows as i32 - 1)).max(rows as i32);
    let column = slot % columns;
    let row = slot / columns;

    let cell_w = usable_w / columns as i32;
    let cell_h = usable_h / rows as i32;
    let extra_w = if column + 1 == columns {
        usable_w - cell_w * columns as i32
    } else {
        0
    };
    let extra_h = if row + 1 == rows {
        usable_h - cell_h * rows as i32
    } else {
        0
    };

    Rect::new(
        viewport.x + column as i32 * (cell_w + gap),
        viewport.y + row as i32 * (cell_h + gap),
        cell_w + extra_w,
        cell_h + extra_h,
    )
}
