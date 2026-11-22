//! VEYRON WM — compositor-independent window management core.
//!
//! The crate is deliberately free of Wayland, GPU and OS dependencies. It owns
//! the parts of the compositor where mistakes are expensive to debug — layout
//! math, workspace state, focus, configuration — so those parts can be verified
//! with plain unit tests.
//!
//! Layering:
//!
//! ```text
//! veyron (smithay, wayland)  ->  veyron-core (this crate)
//!         protocol, input,          layout, workspaces,
//!         rendering, IPC            focus, config, actions
//! ```
//!
//! The core never allocates a window by itself: the compositor reports windows
//! appearing and disappearing and forwards actions, then renders the
//! [`wm::Snapshot`] it gets back.

#![forbid(unsafe_code)]

pub mod action;
pub mod config;
pub mod geometry;
pub mod groups;
pub mod launcher;
pub mod layout;
pub mod motion;
pub mod overview;
pub mod profiles;
pub mod rules;
pub mod session;
pub mod smart;
pub mod snap;
pub mod theme;
pub mod tiling;
pub mod window;
pub mod wm;

pub use action::{Action, ActionParseError, Direction, ResizeAxis, ThemeOp};
pub use config::{Behavior, Binding, Config, ConfigError, Modifiers, WorkspacePreset};
pub use geometry::{Insets, Point, Rect, Size};
pub use groups::{GroupId, GroupSet, WindowGroup};
pub use launcher::{best_match, parse_entry, pick_terminal, split_exec, DesktopEntry};
pub use layout::{LayoutConfig, Placement, ScrollLayout};
pub use motion::{
    workspace_slide, Animator, Easing, MotionConfig, RectAnimator, Spring, SpringConfig,
};
pub use overview::{Overview, OverviewTile, OverviewWindow};
pub use profiles::{LayoutPatch, MotionPatch, Profile, Profiles};
pub use rules::{Glob, RuleSet, WindowPolicy, WindowRule};
pub use session::{Session, WindowRecord};
pub use smart::{Affinities, Affinity};
pub use snap::{PipCorner, SnapZone};
pub use theme::{Design, Palette, ThemeConfig, ThemeMode};
pub use tiling::{arrange, smart_gap, LayoutKind};
pub use window::{NewWindow, Window, WindowId};
pub use wm::{Effects, OutputState, PlacementInfo, Snapshot, Wm, Workspace, WorkspacePosition};
