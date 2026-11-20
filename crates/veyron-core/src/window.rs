//! Window identity and metadata.
//!
//! The core never owns a Wayland surface. It only tracks a [`WindowId`] plus the
//! metadata needed to make management decisions, which keeps the management
//! model completely testable and independent from the protocol layer.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::geometry::{Rect, Size};

/// Stable identifier for a managed window.
///
/// Ids are allocated by [`crate::wm::Wm`] from a monotonically increasing
/// counter and are never reused, so a stale id can always be detected instead of
/// silently addressing a different window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WindowId(pub u64);

impl fmt::Display for WindowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Metadata supplied by the protocol layer when a window is created.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewWindow {
    pub app_id: String,
    pub title: String,
    /// Minimum size requested by the client, if any. Kept for floating
    /// windows; tiled windows are always laid out by the compositor.
    pub min_size: Option<Size>,
}

/// Metadata tracked for a managed window.
#[derive(Debug, Clone)]
pub struct Window {
    id: WindowId,
    app_id: String,
    title: String,
    min_size: Option<Size>,
    floating: bool,
    /// Per-window opacity requested by a rule (`0.0..=1.0`).
    opacity: f32,
    /// Whether a rule asked for a blurred backdrop behind this window.
    blur: bool,
    /// Whether the window lives in the scratchpad layer.
    scratchpad: bool,
    /// Geometry used while the window floats. Tiled geometry is derived from
    /// the layout, never stored here, so the two can never disagree.
    floating_rect: Rect,
    last_tiled_rect: Rect,
}

impl Window {
    pub(crate) fn new(id: WindowId, request: NewWindow) -> Self {
        Self {
            id,
            app_id: request.app_id,
            title: request.title,
            min_size: request.min_size,
            floating: false,
            opacity: 1.0,
            blur: false,
            scratchpad: false,
            floating_rect: Rect::ZERO,
            last_tiled_rect: Rect::ZERO,
        }
    }

    pub fn id(&self) -> WindowId {
        self.id
    }

    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn min_size(&self) -> Option<Size> {
        self.min_size
    }

    pub fn is_floating(&self) -> bool {
        self.floating
    }

    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    pub fn blur(&self) -> bool {
        self.blur
    }

    pub fn is_scratchpad(&self) -> bool {
        self.scratchpad
    }

    pub fn floating_rect(&self) -> Rect {
        self.floating_rect
    }

    pub fn last_tiled_rect(&self) -> Rect {
        self.last_tiled_rect
    }

    pub(crate) fn set_app_id(&mut self, app_id: String) {
        self.app_id = app_id;
    }

    pub(crate) fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub(crate) fn set_floating(&mut self, floating: bool) {
        self.floating = floating;
    }

    pub(crate) fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }

    pub(crate) fn set_blur(&mut self, blur: bool) {
        self.blur = blur;
    }

    pub(crate) fn set_scratchpad(&mut self, scratchpad: bool) {
        self.scratchpad = scratchpad;
    }

    pub(crate) fn set_floating_rect(&mut self, rect: Rect) {
        self.floating_rect = rect;
    }

    pub(crate) fn set_last_tiled_rect(&mut self, rect: Rect) {
        self.last_tiled_rect = rect;
    }
}
