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
pub mod layout;
pub mod window;
pub mod wm;

pub use action::{Action, ActionParseError, Direction};
pub use config::{Binding, Config, ConfigError, Modifiers};
pub use geometry::{Point, Rect, Size};
pub use layout::{LayoutConfig, Placement, ScrollLayout};
pub use window::{NewWindow, Window, WindowId};
pub use wm::{Effects, OutputState, PlacementInfo, Snapshot, Wm, Workspace, WorkspacePosition};
