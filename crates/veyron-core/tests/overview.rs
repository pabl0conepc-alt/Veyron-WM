//! Workspace overview: grid geometry, hit-testing, paging and spatial order.

use veyron_core::{
    Action, Config, Direction, NewWindow, Overview, OverviewConfig, Point, Rect, Size, Wm,
};

fn wm() -> Wm {
    Wm::new(Config::default(), "VEYRON-TEST", Size::new(1280, 800))
}

fn grid(columns: usize) -> OverviewConfig {
    OverviewConfig {
        columns,
        gap: 10,
        margin: 20,
    }
}

#[test]
fn the_grid_fills_the_viewport_without_overlapping_cells() {
    let mut wm = wm();
    wm.dispatch(Action::Workspace { index: 5 });
    let overview = wm.overview(&grid(3));

    assert_eq!(overview.output, Rect::new(0, 0, 1280, 800));
    assert_eq!(overview.viewport, Rect::new(20, 20, 1240, 760));
    assert_eq!(overview.columns, 3);
    assert_eq!(overview.rows, 2);
    assert_eq!(overview.entries.len(), 5);
    assert_eq!((overview.page, overview.pages), (0, 1));

    for entry in &overview.entries {
        assert!(
            overview.viewport.intersection(entry.rect) == Some(entry.rect),
            "cell inside the overview viewport: {:?}",
            entry.rect
        );
        assert!(entry.rect.w > 0 && entry.rect.h > 0);
    }
    for (index, entry) in overview.entries.iter().enumerate() {
        for other in &overview.entries[index + 1..] {
            assert!(!entry.rect.intersects(other.rect), "cells never overlap");
        }
    }

    // The last column and row absorb the rounding remainder, so the grid
    // reaches the viewport edge exactly.
    let right = overview
        .entries
        .iter()
        .map(|entry| entry.rect.right())
        .max()
        .expect("entries");
    let bottom = overview
        .entries
        .iter()
        .map(|entry| entry.rect.bottom())
        .max()
        .expect("entries");
    assert_eq!(right, overview.viewport.right());
    assert_eq!(bottom, overview.viewport.bottom());
}

#[test]
fn hit_testing_maps_a_point_back_to_its_workspace() {
    let mut wm = wm();
    wm.dispatch(Action::Workspace { index: 4 });
    let overview = wm.overview(&grid(2));

    for entry in &overview.entries {
        let centre = Point::new(
            entry.rect.x + entry.rect.w / 2,
            entry.rect.y + entry.rect.h / 2,
        );
        let hit = overview.hit(centre).expect("a cell is under the centre");
        assert_eq!(hit.workspace_index, entry.workspace_index);
        assert_eq!(hit.name, entry.name);
    }

    assert!(overview.hit(Point::new(-5, -5)).is_none());
    assert!(overview
        .hit(Point::new(overview.viewport.right() + 10, 0))
        .is_none());
}

#[test]
fn entries_mirror_the_spatial_desktop_order() {
    let mut wm = wm();
    wm.dispatch(Action::Workspace { index: 2 });
    wm.dispatch(Action::WorkspaceNavigate {
        direction: Direction::Up,
    });
    wm.dispatch(Action::WorkspaceNavigate {
        direction: Direction::Left,
    });
    let overview = wm.overview(&grid(4));

    let positions: Vec<(i64, i64)> = overview
        .entries
        .iter()
        .map(|entry| (entry.position.x, entry.position.y))
        .collect();
    // Created in this order: (0,0), (1,0), then up to (1,-1) and left to (0,-1).
    assert_eq!(positions, vec![(0, -1), (1, -1), (0, 0), (1, 0)]);
    assert!(overview.entries[0].position.y < overview.entries[2].position.y);
}

#[test]
fn a_large_desktop_is_paged_and_opens_on_the_active_workspace() {
    let mut wm = wm();
    wm.dispatch(Action::Workspace { index: 7 });
    let overview = wm.overview(&grid(2));

    assert_eq!(overview.pages, 2);
    assert_eq!(overview.page, 1, "workspace 7 lives on the second page");
    assert_eq!(overview.entries.len(), 3);
    assert!(overview.entries.iter().any(|entry| entry.active));
    assert_eq!(overview.next_page(), Some(0));
    assert_eq!(overview.previous_page(), Some(0));

    let first_page = Overview::build_page(&wm, 0, &grid(2), 0).expect("overview");
    assert_eq!(first_page.page, 0);
    assert_eq!(first_page.entries.len(), 4);
    assert_eq!(first_page.entries[0].workspace_index, 0);
    assert!(first_page
        .entries
        .iter()
        .all(|entry| !entry.active && entry.focused));
    assert_eq!(
        Overview::build_page(&wm, 0, &grid(2), 9)
            .expect("clamped")
            .page,
        1
    );

    let single = {
        let mut wm = wm;
        wm.dispatch(Action::Workspace { index: 1 });
        wm.overview(&grid(4))
    };
    assert_eq!(single.next_page(), None);
    assert_eq!(single.previous_page(), None);
}

#[test]
fn window_counts_and_active_state_are_reported() {
    let mut wm = wm();
    let tiled = wm.create_window(NewWindow::default());
    wm.dispatch(Action::ToggleFloating);
    let floating = wm.create_window(NewWindow::default());
    wm.dispatch(Action::Workspace { index: 2 });

    let overview = wm.overview(&grid(2));
    let first = overview.entry(0).expect("first workspace");
    assert_eq!(first.windows, 2);
    assert!(!first.active);
    assert_eq!(first.name, "1");
    assert_eq!(first.rect, overview.entries[0].rect);

    let second = overview.entry(1).expect("second workspace");
    assert_eq!(second.windows, 0);
    assert!(second.active);

    // Both windows are still tracked by the core.
    assert!(wm.window(tiled).is_some());
    assert!(wm.window(floating).is_some());
}

#[test]
fn the_overview_only_covers_the_requested_output() {
    let mut wm = wm();
    wm.create_window(NewWindow::default());
    wm.add_output("HDMI-A-1", Rect::new(1280, 0, 1920, 1080));

    let primary = Overview::build(&wm, 0, &grid(2)).expect("first output");
    assert_eq!(primary.output_index, 0);
    assert_eq!(primary.entries.len(), 1);

    let secondary = Overview::build(&wm, 1, &grid(2)).expect("second output");
    assert_eq!(secondary.output, Rect::new(1280, 0, 1920, 1080));
    assert_eq!(secondary.viewport.x, 1300);
    assert_eq!(secondary.entries.len(), 1);
    assert!(
        secondary.entries[0].active,
        "each output shows its own workspace"
    );
    assert!(!secondary.entries[0].focused, "the first output has focus");
    assert!(primary.entries[0].focused);
    assert!(Overview::build(&wm, 5, &grid(2)).is_none());
}
