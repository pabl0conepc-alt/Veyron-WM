//! Multi-output behaviour: hot-plug, per-output viewports and moving windows
//! or whole workspaces between monitors.

use veyron_core::{Action, Config, Direction, NewWindow, Rect, Size, Wm};

fn wm() -> Wm {
    Wm::new(Config::default(), "eDP-1", Size::new(1280, 800))
}

fn secondary() -> Rect {
    Rect::new(1280, 0, 1920, 1080)
}

#[test]
fn hotplugging_an_output_keeps_focus_and_starts_with_workspace_one() {
    let mut wm = wm();
    let id = wm.create_window(NewWindow::default());

    let index = wm
        .add_output("HDMI-A-1", secondary())
        .expect("output added");
    assert_eq!(index, 1);
    assert_eq!(wm.outputs().len(), 2);
    assert_eq!(wm.active_output_index(), 0);
    assert_eq!(wm.snapshot().focused, Some(id));

    let added = &wm.outputs()[1];
    assert_eq!(added.name(), "HDMI-A-1");
    assert_eq!(added.rect(), secondary());
    assert_eq!(added.workspaces().len(), 1);
    assert_eq!(added.active_workspace().name(), "1");
    assert_eq!(wm.output_index("HDMI-A-1"), Some(1));
    assert!(wm.output("eDP-1").is_some());
    assert!(wm.output("DP-9").is_none());

    // Duplicate names and empty rectangles are refused.
    assert!(wm
        .add_output("HDMI-A-1", Rect::new(0, 0, 800, 600))
        .is_none());
    assert!(wm.add_output("  ", Rect::new(0, 0, 800, 600)).is_none());
    assert!(wm.add_output("DP-1", Rect::ZERO).is_none());
    assert_eq!(wm.outputs().len(), 2);
}

#[test]
fn every_output_gets_its_own_viewport_and_snapshot() {
    let mut wm = wm();
    wm.add_output("HDMI-A-1", secondary());
    assert_eq!(wm.viewport_of(0), Rect::new(12, 12, 1256, 776));
    assert_eq!(wm.viewport_of(1), Rect::new(1292, 12, 1896, 1056));
    assert_eq!(wm.viewport_of(9), Rect::ZERO);

    assert!(wm.focus_output(1));
    let id = wm.create_window(NewWindow::default());
    let snapshot = wm.snapshot();
    assert_eq!(snapshot.output_name, "HDMI-A-1");
    assert_eq!(snapshot.output_index, 1);
    assert_eq!(snapshot.output, secondary());
    let rect = snapshot.placement_of(id).expect("placement").rect;
    assert!(rect.x >= snapshot.viewport.x);
    assert!(rect.right() <= snapshot.viewport.right());

    let all = wm.snapshots();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].output_name, "eDP-1");
    assert!(all[0].placements.is_empty());
    assert_eq!(all[1].placements.len(), 1);
    assert!(wm.output_snapshot(7).is_none());
}

#[test]
fn resizing_one_output_leaves_the_other_alone() {
    let mut wm = wm();
    let first = wm.create_window(NewWindow::default());
    wm.add_output("HDMI-A-1", secondary());
    wm.focus_output(1);
    let second = wm.create_window(NewWindow::default());
    wm.dispatch(Action::ToggleFloating);

    let before = wm
        .output_snapshot(0)
        .expect("first output")
        .placement_of(first)
        .expect("placement")
        .rect;
    assert!(wm.set_output_rect(1, Rect::new(1280, 0, 640, 480)));
    assert!(!wm.set_output_rect(1, Rect::new(1280, 0, 640, 480)));

    let first_snapshot = wm.output_snapshot(0).expect("first output");
    assert_eq!(
        first_snapshot.placement_of(first).expect("placement").rect,
        before
    );

    let second_snapshot = wm.output_snapshot(1).expect("second output");
    assert_eq!(second_snapshot.output, Rect::new(1280, 0, 640, 480));
    let rect = second_snapshot
        .placement_of(second)
        .expect("floating placement")
        .rect;
    assert!(
        second_snapshot.viewport.intersection(rect) == Some(rect),
        "a floating window stays inside its own output: {rect:?}"
    );
}

#[test]
fn output_focus_follows_screen_directions() {
    let mut wm = wm();
    wm.add_output("HDMI-A-1", secondary());

    assert!(
        wm.dispatch(Action::OutputFocus {
            direction: Direction::Right
        })
        .focus_changed
    );
    assert_eq!(wm.active_output_index(), 1);
    assert!(
        !wm.dispatch(Action::OutputFocus {
            direction: Direction::Right
        })
        .focus_changed
    );
    assert!(
        !wm.dispatch(Action::OutputFocus {
            direction: Direction::Up
        })
        .focus_changed
    );
    assert!(
        wm.dispatch(Action::OutputFocus {
            direction: Direction::Left
        })
        .focus_changed
    );
    assert_eq!(wm.active_output_index(), 0);

    // New windows open on the active output.
    let id = wm.create_window(NewWindow::default());
    assert_eq!(wm.snapshot().focused, Some(id));
    assert_eq!(wm.snapshot().placements.len(), 1);
    assert!(wm.output_snapshot(1).expect("second").placements.is_empty());
}

#[test]
fn moving_a_window_between_outputs_keeps_it_visible_and_focused() {
    let mut wm = wm();
    let id = wm.create_window(NewWindow::default());
    wm.add_output("HDMI-A-1", secondary());

    assert!(
        wm.dispatch(Action::MoveToOutput {
            direction: Direction::Right
        })
        .workspace_changed
    );
    assert_eq!(wm.active_output_index(), 1);

    let snapshot = wm.snapshot();
    assert_eq!(snapshot.focused, Some(id));
    let rect = snapshot.placement_of(id).expect("placement").rect;
    assert!(rect.x >= snapshot.viewport.x);
    assert!(rect.right() <= snapshot.viewport.right());
    assert!(wm.output_snapshot(0).expect("first").placements.is_empty());

    // The viewport of the old output is unchanged by the move.
    assert_eq!(
        wm.output_snapshot(0).expect("first").viewport,
        Rect::new(12, 12, 1256, 776)
    );
}

#[test]
fn moving_a_floating_window_reclamps_it_to_the_new_output() {
    let mut wm = wm();
    let id = wm.create_window(NewWindow::default());
    wm.dispatch(Action::ToggleFloating);
    wm.add_output("HDMI-A-1", secondary());
    wm.focus_output(0);
    wm.focus_window(id);

    assert!(
        wm.dispatch(Action::MoveToOutput {
            direction: Direction::Right
        })
        .workspace_changed
    );
    let snapshot = wm.snapshot();
    assert!(snapshot.is_floating(id));
    let rect = snapshot.placement_of(id).expect("placement").rect;
    assert!(
        snapshot.viewport.intersection(rect) == Some(rect),
        "floating window fits the destination viewport"
    );
}

#[test]
fn moving_the_active_workspace_to_an_output_takes_its_windows() {
    let mut wm = wm();
    let id = wm.create_window(NewWindow::default());
    wm.dispatch(Action::Workspace { index: 2 });
    wm.add_output("HDMI-A-1", secondary());

    assert!(
        wm.dispatch(Action::WorkspaceToOutput {
            direction: Direction::Right
        })
        .workspace_changed
    );
    assert_eq!(wm.active_output_index(), 1);
    assert_eq!(wm.snapshot().workspace, "2");
    assert_eq!(wm.outputs()[0].workspaces().len(), 1);
    assert_eq!(wm.outputs()[1].workspaces().len(), 2);

    // Workspace 1 keeps its window on the first output.
    assert!(wm.focus_output(0));
    assert_eq!(wm.snapshot().workspace, "1");
    assert_eq!(wm.snapshot().focused, Some(id));

    // The names follow the workspace, not the output.
    assert_eq!(wm.outputs()[1].active_workspace().name(), "2");
    assert_eq!(wm.outputs()[1].workspaces().len(), 2);
}

#[test]
fn a_workspace_that_the_target_already_has_is_not_moved() {
    let mut wm = wm();
    wm.create_window(NewWindow::default());
    wm.add_output("HDMI-A-1", secondary());

    assert!(
        !wm.dispatch(Action::WorkspaceToOutput {
            direction: Direction::Right
        })
        .workspace_changed
    );
    assert_eq!(wm.active_output_index(), 0);
    assert_eq!(wm.outputs()[0].workspaces().len(), 1);
    assert_eq!(wm.outputs()[1].workspaces().len(), 1);
}

#[test]
fn unplugging_an_output_migrates_its_windows_to_the_remaining_one() {
    let mut wm = wm();
    wm.add_output("HDMI-A-1", secondary());
    wm.focus_output(1);

    let floating = wm.create_window(NewWindow {
        app_id: "mpv".into(),
        ..NewWindow::default()
    });
    wm.dispatch(Action::ToggleFloating);
    let tiled = wm.create_window(NewWindow::default());

    assert!(wm.remove_output("HDMI-A-1"));
    assert_eq!(wm.outputs().len(), 1);
    assert_eq!(wm.active_output_index(), 0);
    assert!(
        !wm.remove_output("eDP-1"),
        "the last output is never removed"
    );
    assert!(!wm.remove_output("nope"));

    let snapshot = wm.snapshot();
    assert_eq!(snapshot.placements.len(), 2);
    for id in [floating, tiled] {
        let rect = snapshot.placement_of(id).expect("window migrated").rect;
        assert!(
            snapshot.viewport.intersection(rect) == Some(rect),
            "migrated window fits the remaining output: {rect:?}"
        );
    }
    assert!(snapshot.is_floating(floating));
    assert!(!snapshot.is_floating(tiled));
}
