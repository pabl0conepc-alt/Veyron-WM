//! Veyron Profiles: configuration parsing and runtime layout/motion presets.

use veyron_core::{Action, Config, LayoutConfig, NewWindow, Profile, Profiles, Size, Wm};

const CONFIG: &str = r#"
default_profile = "coding"

[layout]
gap = 8
mode = "scroll"

[[profiles]]
name = "coding"
description = "dense master-stack for editing"
[profiles.layout]
gap = 4
mode = "master-stack"
smart_gaps = true
[profiles.motion]
enabled = true
duration_ms = 120

[[profiles]]
name = "cinema"
[profiles.layout]
gap = 24
outer_margin = 32
mode = "scroll"
"#;

fn config() -> Config {
    Config::from_toml_str(CONFIG).expect("valid configuration")
}

fn wm() -> Wm {
    Wm::new(config(), "VEYRON-TEST", Size::new(1280, 800))
}

#[test]
fn profiles_are_parsed_with_names_and_patches() {
    let config = config();
    assert_eq!(config.profiles.len(), 2);
    assert_eq!(config.default_profile.as_deref(), Some("coding"));
    assert_eq!(
        config.profiles.names().collect::<Vec<_>>(),
        vec!["coding", "cinema"]
    );

    let coding = config.profiles.get("coding").expect("profile");
    assert_eq!(coding.layout.gap, Some(4));
    assert!(coding.layout.smart_gaps.unwrap_or(false));
    assert_eq!(coding.motion.duration_ms, Some(120));

    let cinema = config.profiles.get("cinema").expect("profile");
    assert_eq!(cinema.layout.outer_margin, Some(32));
    assert!(cinema.motion.is_empty());
    assert!(config.profiles.get("missing").is_none());
}

#[test]
fn invalid_profile_sets_are_rejected() {
    let duplicate =
        Config::from_toml_str("[[profiles]]\nname = \"a\"\n[[profiles]]\nname = \"a\"\n");
    assert!(duplicate.is_err(), "duplicate names are refused");

    let missing_default = Config::from_toml_str("default_profile = \"nope\"\n");
    assert!(missing_default.is_err(), "the default profile must exist");

    let empty_name = Config::from_toml_str("[[profiles]]\ndescription = \"x\"\n");
    assert!(empty_name.is_err());

    let spaced = Config::from_toml_str("[[profiles]]\nname = \"two words\"\n");
    assert!(spaced.is_err(), "the selector is a single token");

    let negative_gap =
        Config::from_toml_str("[[profiles]]\nname = \"a\"\n[profiles.layout]\ngap = -2\n");
    assert!(negative_gap.is_err());
}

#[test]
fn the_default_profile_is_applied_at_startup() {
    let wm = wm();
    assert_eq!(wm.active_profile(), Some("coding"));
    assert_eq!(wm.config().layout.gap, 4);
    assert!(wm.config().layout.smart_gaps);
    assert_eq!(wm.config().motion.duration_ms, 120);
    assert!(wm.config().motion.enabled);

    let snapshot = wm.snapshot();
    assert_eq!(snapshot.layout, veyron_core::LayoutKind::MasterStack);
}

#[test]
fn the_profile_gap_reaches_the_snapshot() {
    let mut wm = wm();
    wm.create_window(NewWindow::default());
    wm.create_window(NewWindow::default());
    assert_eq!(wm.snapshot().gap, 4);
}

#[test]
fn applying_a_profile_retiles_every_workspace_and_keeps_unset_fields() {
    let mut wm = Wm::new(
        Config::from_toml_str(CONFIG).expect("valid"),
        "VEYRON-TEST",
        Size::new(1280, 800),
    );
    let first = wm.create_window(NewWindow::default());
    wm.dispatch(Action::Workspace { index: 2 });
    let second = wm.create_window(NewWindow::default());

    assert!(
        wm.dispatch(Action::Profile {
            name: "cinema".into()
        })
        .layout_changed
    );
    assert_eq!(wm.active_profile(), Some("cinema"));
    assert_eq!(wm.config().layout.gap, 24);
    assert_eq!(wm.config().layout.outer_margin, 32);
    // The coding profile's smart gaps are not silently kept or reset blindly:
    // cinema does not patch it, so the previous value stays.
    assert!(wm.config().layout.smart_gaps);
    // Unset ratios keep their configured value.
    assert_eq!(
        wm.config().layout.master_ratio,
        LayoutConfig::default().master_ratio
    );

    let viewport = wm.viewport();
    assert_eq!(viewport.x, 32);
    let snapshot = wm.snapshot();
    assert_eq!(
        snapshot.layout,
        veyron_core::LayoutKind::Scroll,
        "cinema selects the scrolling layout again"
    );
    let rect = snapshot.placement_of(second).expect("placement").rect;
    assert!(rect.x >= viewport.x);
    assert!(rect.right() <= viewport.right());

    wm.dispatch(Action::Workspace { index: 1 });
    let snapshot = wm.snapshot();
    assert_eq!(snapshot.focused, Some(first));
    assert_eq!(snapshot.placements.len(), 1);
}

#[test]
fn unknown_profiles_change_nothing() {
    let mut wm = wm();
    let before = wm.config().layout.gap;
    assert!(
        !wm.dispatch(Action::Profile {
            name: "does-not-exist".into()
        })
        .layout_changed
    );
    assert_eq!(wm.active_profile(), Some("coding"));
    assert_eq!(wm.config().layout.gap, before);
}

#[test]
fn a_profile_that_would_break_the_layout_is_refused() {
    let mut config = Config::from_toml_str(
        "[layout]\ndefault_column_ratio = 0.4\n\n[[profiles]]\nname = \"wide\"\n",
    )
    .expect("valid");
    config.profiles = Profiles::new(vec![Profile {
        name: "wide".into(),
        layout: veyron_core::LayoutPatch {
            min_column_ratio: Some(0.5),
            ..Default::default()
        },
        ..Profile::default()
    }])
    .expect("patch is valid on its own");

    let mut wm = Wm::new(config, "VEYRON-TEST", Size::new(1280, 800));
    let before = wm.config().layout.min_column_ratio;
    assert!(!wm.apply_profile("wide"));
    assert_eq!(wm.active_profile(), None);
    assert_eq!(wm.config().layout.min_column_ratio, before);
}

#[test]
fn a_profile_can_take_an_outer_margin_to_zero_on_every_output() {
    let mut wm = wm();
    wm.add_output("HDMI-A-1", veyron_core::Rect::new(1280, 0, 1920, 1080));
    assert!(
        wm.dispatch(Action::Profile {
            name: "cinema".into()
        })
        .layout_changed
    );
    for index in 0..2 {
        let viewport = wm.viewport_of(index);
        assert!(viewport.x >= 32);
        assert!(viewport.y >= 32);
    }
    assert_eq!(
        wm.viewport_of(0),
        wm.output_snapshot(0).expect("first").viewport
    );
}

#[test]
fn profiles_survive_a_config_round_trip() {
    let config = config();
    let text = config.to_toml_string().expect("serializable");
    let reparsed = Config::from_toml_str(&text).expect("reparse");
    assert_eq!(reparsed.profiles.len(), 2);
    assert_eq!(reparsed.default_profile.as_deref(), Some("coding"));
    assert_eq!(
        reparsed.profiles.get("coding").expect("profile").layout.gap,
        Some(4)
    );
}
