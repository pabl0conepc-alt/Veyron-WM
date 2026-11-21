//! Workspace overview geometry.
//!
//! An exposé-style overview shows every workspace at once, laid out on the same
//! sparse grid the user navigates. The geometry lives here so it can be unit
//! tested without a renderer; the compositor exposes it over the control
//! protocol and an overview surface can draw the same tiles directly.

use serde::{Deserialize, Serialize};

use crate::{
    geometry::{Rect, Size},
    wm::{Wm, WorkspacePosition},
};

/// One workspace tile in the overview grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverviewTile {
    pub workspace: String,
    pub index: usize,
    pub position: WorkspacePosition,
    pub active: bool,
    /// Tile rectangle in overview coordinates.
    pub rect: Rect,
    /// Windows of that workspace, scaled into the tile.
    pub windows: Vec<OverviewWindow>,
}

/// A window drawn inside an overview tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverviewWindow {
    pub window: u64,
    pub rect: Rect,
    pub floating: bool,
}

/// Every workspace of the active output in one grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Overview {
    pub tiles: Vec<OverviewTile>,
    /// Region that covers every tile.
    pub bounds: Rect,
    /// Output area the window rectangles are scaled from.
    pub viewport: Rect,
}

impl Wm {
    /// Lay out every workspace of the active output on a grid.
    ///
    /// `tile` is the size of one workspace tile and `gap` the space between
    /// tiles. Tiles keep their relative sparse-grid position; coordinates are
    /// normalised so the top-left tile starts at the origin. Window rectangles
    /// are scaled from the output viewport into their tile, so an overview
    /// client only has to copy numbers.
    pub fn overview(&self, tile: Size, gap: i32) -> Overview {
        let output_index = self.active_output_index();
        let output = self.active_output();
        let viewport = self.viewport();
        let origin_x = output
            .workspaces()
            .iter()
            .map(|workspace| workspace.position().x)
            .min()
            .unwrap_or(0);
        let origin_y = output
            .workspaces()
            .iter()
            .map(|workspace| workspace.position().y)
            .min()
            .unwrap_or(0);

        let mut tiles = Vec::with_capacity(output.workspaces().len());
        for (index, workspace) in output.workspaces().iter().enumerate() {
            let cell_x = (workspace.position().x - origin_x) as i32;
            let cell_y = (workspace.position().y - origin_y) as i32;
            let rect = Rect::new(
                cell_x.saturating_mul(tile.w + gap),
                cell_y.saturating_mul(tile.h + gap),
                tile.w,
                tile.h,
            );

            let mut windows = Vec::new();
            if let Some(snapshot) = self.workspace_snapshot(output_index, index) {
                windows.extend(snapshot.placements.iter().map(|placement| OverviewWindow {
                    window: placement.window.0,
                    rect: scale_into(placement.rect, viewport, rect),
                    floating: placement.floating,
                }));
            }

            tiles.push(OverviewTile {
                workspace: workspace.name().to_string(),
                index,
                position: workspace.position(),
                active: index == output.active_workspace_index(),
                rect,
                windows,
            });
        }

        let bounds = tiles
            .iter()
            .fold(Rect::ZERO, |bounds, tile| union(bounds, tile.rect));
        Overview {
            tiles,
            bounds,
            viewport,
        }
    }
}

/// Scale `source` from `from` into `into`, keeping its relative position.
fn scale_into(source: Rect, from: Rect, into: Rect) -> Rect {
    if from.w <= 0 || from.h <= 0 {
        return Rect::ZERO;
    }
    let scale_x = |value: i32| (value as i64 * into.w as i64 / from.w as i64) as i32;
    let scale_y = |value: i32| (value as i64 * into.h as i64 / from.h as i64) as i32;
    Rect::new(
        into.x + scale_x(source.x - from.x),
        into.y + scale_y(source.y - from.y),
        scale_x(source.w).max(1),
        scale_y(source.h).max(1),
    )
}

/// Smallest rectangle covering both inputs; an empty input is ignored.
fn union(a: Rect, b: Rect) -> Rect {
    if a.is_empty() {
        return b;
    }
    if b.is_empty() {
        return a;
    }
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Rect::new(
        x,
        y,
        a.right().max(b.right()) - x,
        a.bottom().max(b.bottom()) - y,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        action::{Action, Direction},
        config::Config,
        window::NewWindow,
    };

    fn wm() -> Wm {
        Wm::new(Config::default(), "VEYRON-TEST", Size::new(1280, 800))
    }

    fn tile<'a>(overview: &'a Overview, workspace: &str) -> &'a OverviewTile {
        overview
            .tiles
            .iter()
            .find(|tile| tile.workspace == workspace)
            .expect("workspace tile")
    }

    #[test]
    fn tiles_follow_the_sparse_grid_and_are_normalised() {
        let mut wm = wm();
        wm.create_window(NewWindow::default());
        wm.dispatch(Action::Workspace { index: 2 });
        wm.dispatch(Action::WorkspaceNavigate {
            direction: Direction::Up,
        });

        let overview = wm.overview(Size::new(200, 120), 10);
        assert_eq!(overview.tiles.len(), 3);
        assert_eq!(tile(&overview, "1").rect, Rect::new(0, 130, 200, 120));
        assert_eq!(tile(&overview, "2").rect, Rect::new(210, 130, 200, 120));
        assert_eq!(
            tile(&overview, "3").rect,
            Rect::new(210, 0, 200, 120),
            "a workspace above the others starts the grid at y = 0"
        );
        assert_eq!(overview.bounds, Rect::new(0, 0, 410, 250));
        assert!(tile(&overview, "3").active);
        assert_eq!(overview.tiles.iter().filter(|tile| tile.active).count(), 1);
    }

    #[test]
    fn windows_are_scaled_inside_their_tile() {
        let mut wm = wm();
        wm.create_window(NewWindow::default());
        wm.create_window(NewWindow::default());
        wm.dispatch(Action::Workspace { index: 2 });

        let overview = wm.overview(Size::new(200, 120), 10);
        let first = tile(&overview, "1");
        assert_eq!(first.windows.len(), 2);
        for window in &first.windows {
            assert!(window.rect.x >= first.rect.x && window.rect.y >= first.rect.y);
            assert!(window.rect.right() <= first.rect.right());
            assert!(window.rect.bottom() <= first.rect.bottom());
            assert!(window.rect.w >= 1 && window.rect.h >= 1);
        }
        assert!(
            tile(&overview, "2").windows.is_empty(),
            "an empty workspace still gets a tile"
        );
    }
}
