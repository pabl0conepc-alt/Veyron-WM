//! Smart workspace placement and the canvas gesture, exercised through the
//! public core API only.

use veyron_core::{Action, Config, NewWindow, Size, WindowId, Wm};

const OUTPUT: &str = "VEYRON-TEST";

fn config_with(smart_workspace: bool) -> Config {
    let mut config = Config::default();
    config.behavior.smart_workspace = smart_workspace;
    config
}

fn wm_with(smart_workspace: bool) -> Wm {
    Wm::new(config_with(smart_workspace), OUTPUT, Size::new(1280, 800))
}

fn open(wm: &mut Wm, app_id: &str) -> WindowId {
    wm.create_window(NewWindow {
        app_id: app_id.to_string(),
        title: app_id.to_string(),
        ..NewWindow::default()
    })
}

fn active_workspace(wm: &Wm) -> String {
    wm.active_output().active_workspace().name().to_string()
}

fn workspace_of(wm: &Wm, windows: usize, id: WindowId) -> Option<String> {
    let output = wm.active_output();
    (0..windows).find_map(|index| {
        let snapshot = wm.workspace_snapshot(0, index)?;
        snapshot.placement_of(id).map(|_| {
            output
                .workspaces()
                .get(index)
                .map(|workspace| workspace.name().to_string())
                .unwrap_or_default()
        })
    })
}

#[test]
fn smart_workspace_is_off_by_default() {
    let mut wm = wm_with(false);

    wm.dispatch(Action::Workspace { index: 2 });
    let first = open(&mut wm, "firefox");
    assert_eq!(workspace_of(&wm, 2, first).as_deref(), Some("2"));

    wm.dispatch(Action::Workspace { index: 1 });
    let second = open(&mut wm, "firefox");
    assert_eq!(
        workspace_of(&wm, 2, second).as_deref(),
        Some("1"),
        "without smart workspace a new window opens where the user is"
    );
}

#[test]
fn smart_workspace_joins_the_app_workspace_without_switching() {
    let mut wm = wm_with(true);

    wm.dispatch(Action::Workspace { index: 2 });
    let first = open(&mut wm, "firefox");
    assert_eq!(workspace_of(&wm, 2, first).as_deref(), Some("2"));

    wm.dispatch(Action::Workspace { index: 1 });
    assert_eq!(active_workspace(&wm), "1");
    let second = open(&mut wm, "firefox");

    assert_eq!(
        workspace_of(&wm, 2, second).as_deref(),
        Some("2"),
        "the new window joins its siblings"
    );
    assert_eq!(
        active_workspace(&wm),
        "1",
        "placement does not yank the user to another workspace"
    );
}

#[test]
fn smart_workspace_follows_a_window_moved_manually() {
    let mut wm = wm_with(true);
    let first = open(&mut wm, "firefox");

    assert!(
        wm.dispatch(Action::MoveToWorkspace {
            direction: veyron_core::Direction::Right,
        })
        .workspace_changed
    );
    assert_eq!(workspace_of(&wm, 2, first).as_deref(), Some("2"));

    wm.dispatch(Action::Workspace { index: 3 });
    let second = open(&mut wm, "firefox");

    assert_eq!(
        workspace_of(&wm, 3, second).as_deref(),
        Some("2"),
        "new windows should follow the app's latest manually chosen workspace"
    );
}

#[test]
fn smart_workspace_learns_a_late_client_identity() {
    let mut wm = wm_with(true);
    let first = wm.create_window(NewWindow::default());
    wm.set_window_identity(first, "firefox", "Start Page");

    wm.dispatch(Action::Workspace { index: 2 });
    let second = open(&mut wm, "firefox");

    assert_eq!(
        workspace_of(&wm, 2, second).as_deref(),
        Some("1"),
        "identity announced after toplevel creation should seed app placement"
    );
}

#[test]
fn unrelated_apps_are_not_affected_by_the_learning() {
    let mut wm = wm_with(true);
    wm.dispatch(Action::Workspace { index: 2 });
    open(&mut wm, "firefox");
    wm.dispatch(Action::Workspace { index: 1 });

    let mpv = open(&mut wm, "mpv");
    assert_eq!(workspace_of(&wm, 2, mpv).as_deref(), Some("1"));
}

#[test]
fn anonymous_windows_are_never_learned() {
    let mut wm = wm_with(true);
    let terminal = open(&mut wm, "");
    assert_eq!(workspace_of(&wm, 1, terminal).as_deref(), Some("1"));
    assert!(wm.window(terminal).expect("window").app_id().is_empty());
}

#[test]
fn canvas_pan_moves_every_floating_window_together() {
    let mut wm = wm_with(true);
    let first = open(&mut wm, "a");
    assert!(wm.dispatch(Action::ToggleFloating).layout_changed);
    let second = open(&mut wm, "b");
    assert!(wm.dispatch(Action::ToggleFloating).layout_changed);

    let before = [first, second].map(|id| wm.snapshot().placement_of(id).expect("placement").rect);

    assert!(
        wm.dispatch(Action::CanvasPan { dx: 40, dy: 0 })
            .layout_changed
    );

    for (id, old) in [first, second].into_iter().zip(before) {
        let now = wm.snapshot().placement_of(id).expect("placement").rect;
        assert_eq!(now.x, old.x + 40);
        assert_eq!(now.y, old.y);
        assert_eq!(now.size(), old.size());
    }

    // Back at the left edge, panning further left cannot move them.
    assert!(
        wm.dispatch(Action::CanvasPan { dx: -40, dy: 0 })
            .layout_changed
    );
    assert!(
        !wm.dispatch(Action::CanvasPan { dx: -40, dy: 0 })
            .layout_changed
    );
}

#[test]
fn canvas_pan_does_nothing_without_floating_windows() {
    let mut wm = wm_with(false);
    open(&mut wm, "a");
    assert!(
        !wm.dispatch(Action::CanvasPan { dx: 40, dy: 40 })
            .layout_changed
    );
}

#[test]
fn canvas_action_round_trips_and_rejects_zero() {
    let action = Action::parse("canvas -40 24").expect("parse");
    assert_eq!(action, Action::CanvasPan { dx: -40, dy: 24 });
    assert_eq!(action.to_string(), "canvas -40 24");
    assert!(Action::parse("canvas 0 0").is_err());
    assert!(Action::parse("canvas 10").is_err());
    assert!(Action::parse("canvas 10 20 30").is_err());
}
