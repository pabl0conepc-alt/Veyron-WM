//! End-to-end checks for the window-management features that need more than one
//! module: snapping, picture-in-picture, groups and smart focus.

use veyron_core::{
    action::Action,
    config::Config,
    geometry::Size,
    snap::{PipCorner, SnapZone},
    window::{NewWindow, WindowId},
    wm::Wm,
};

fn wm() -> Wm {
    Wm::new(Config::default(), "VEYRON-TEST", Size::new(1200, 800))
}

fn open(wm: &mut Wm, app_id: &str) -> WindowId {
    wm.create_window(NewWindow {
        app_id: app_id.to_string(),
        ..NewWindow::default()
    })
}

#[test]
fn snap_floats_the_focused_window_into_a_zone() {
    let mut wm = wm();
    let term = open(&mut wm, "foot");
    let gap = wm.config().layout.gap;

    let effects = wm.dispatch(Action::Snap {
        zone: SnapZone::Left,
    });
    assert!(effects.layout_changed);

    let snapshot = wm.snapshot();
    let placement = snapshot
        .placement_of(term)
        .expect("snapped window is placed");
    assert!(placement.floating, "snapping floats a tiled window");
    assert_eq!(placement.rect, SnapZone::Left.rect(snapshot.viewport, gap));

    wm.dispatch(Action::Snap {
        zone: SnapZone::Right,
    });
    let snapshot = wm.snapshot();
    let placement = snapshot.placement_of(term).expect("placed");
    assert_eq!(placement.rect, SnapZone::Right.rect(snapshot.viewport, gap));
}

#[test]
fn picture_in_picture_docks_and_restores_the_previous_geometry() {
    let mut wm = wm();
    let video = open(&mut wm, "mpv");
    wm.dispatch(Action::ToggleFloating);
    let before = wm
        .snapshot()
        .placement_of(video)
        .map(|placement| placement.rect)
        .expect("floating window is placed");

    wm.dispatch(Action::Pip {
        corner: PipCorner::BottomRight,
    });
    let snapshot = wm.snapshot();
    let placement = snapshot.placement_of(video).expect("placed");
    assert!(placement.pip);
    assert_eq!(
        placement.rect,
        PipCorner::BottomRight.rect(snapshot.viewport, veyron_core::snap::DEFAULT_PIP_RATIO, 0)
    );
    assert!(wm.is_pip(video));

    wm.dispatch(Action::Pip {
        corner: PipCorner::BottomRight,
    });
    let snapshot = wm.snapshot();
    let placement = snapshot.placement_of(video).expect("placed");
    assert!(!placement.pip);
    assert_eq!(
        placement.rect, before,
        "leaving picture-in-picture restores"
    );
    assert!(!wm.is_pip(video));
}

#[test]
fn grouping_keeps_members_adjacent_and_cycles_focus() {
    let mut wm = wm();
    let first = open(&mut wm, "foot");
    let second = open(&mut wm, "firefox");
    let third = open(&mut wm, "signal");

    // The focused window joins the group of the window focused before it.
    let effects = wm.dispatch(Action::GroupToggle);
    assert!(effects.layout_changed);
    let group = wm.group_of(third).expect("the window is grouped");
    assert_eq!(wm.group_of(second), Some(group));
    assert_eq!(wm.group_of(first), None);

    let order: Vec<WindowId> = wm
        .snapshot()
        .placements
        .iter()
        .map(|placement| placement.window)
        .collect();
    let second_at = order.iter().position(|id| *id == second).expect("placed");
    assert_eq!(
        order.get(second_at + 1),
        Some(&third),
        "group members sit next to each other"
    );

    let effects = wm.dispatch(Action::GroupCycle { delta: 1 });
    assert!(effects.focus_changed);
    assert_eq!(wm.snapshot().focused, Some(second));
    wm.dispatch(Action::GroupCycle { delta: 1 });
    assert_eq!(wm.snapshot().focused, Some(third));

    // Grouping is a toggle: the focused window leaves its group.
    let effects = wm.dispatch(Action::GroupToggle);
    assert!(effects.layout_changed);
    assert_eq!(wm.group_of(third), None);
    assert_eq!(wm.group_of(second), Some(group), "the other member stays");
    assert_eq!(wm.groups().len(), 1);
    assert_eq!(
        wm.groups().groups()[0].members(),
        &[second],
        "the group keeps only its remaining member"
    );
}

#[test]
fn smart_focus_toggles_between_the_two_most_recent_windows() {
    let mut wm = wm();
    let first = open(&mut wm, "foot");
    let second = open(&mut wm, "firefox");
    let third = open(&mut wm, "signal");
    assert_eq!(wm.snapshot().focused, Some(third));

    wm.dispatch(Action::FocusPrevious);
    assert_eq!(wm.snapshot().focused, Some(second));
    wm.dispatch(Action::FocusPrevious);
    assert_eq!(wm.snapshot().focused, Some(third));

    assert!(wm.focus_history().contains(&first));
    wm.remove_window(third);
    wm.dispatch(Action::FocusPrevious);
    assert_ne!(wm.snapshot().focused, Some(third));
}

#[test]
fn groups_forget_closed_windows() {
    let mut wm = wm();
    let term = open(&mut wm, "foot");
    let editor = open(&mut wm, "code");
    wm.dispatch(Action::GroupToggle);
    let group = wm.group_of(editor).expect("grouped");

    assert!(wm.remove_window(editor));
    assert_eq!(wm.group_of(term), Some(group), "the live member keeps it");
    assert_eq!(wm.groups().groups()[0].members(), &[term]);

    assert!(wm.remove_window(term));
    assert!(wm.groups().is_empty(), "no group survives its last window");
}
