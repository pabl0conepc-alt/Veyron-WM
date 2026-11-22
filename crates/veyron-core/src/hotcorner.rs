//! Hot corners: an action runs when the pointer is pushed into a corner of
//! the output, like GNOME's activities corner.
//!
//! ```toml
//! [hot_corners]
//! top_left = "focus-cycle next"
//! bottom_right = "spawn foot"
//! ```

use serde::{Deserialize, Serialize};

use crate::{action::Action, config::ConfigError, geometry::Rect};

/// How close to a corner, in logical pixels, the pointer must come.
pub const CORNER_SIZE: i32 = 2;
/// How far the pointer must leave a corner before it can fire again, so
/// jitter at the edge does not repeat the action.
pub const REARM_DISTANCE: i32 = 24;

/// A corner of the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub const ALL: [Corner; 4] = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ];

    /// The corner of `output` within `reach` pixels of `(x, y)`, if any.
    pub fn at(output: Rect, x: f64, y: f64, reach: i32) -> Option<Corner> {
        let reach = f64::from(reach.max(0));
        let left = x < f64::from(output.x) + reach;
        let right = x >= f64::from(output.right()) - reach;
        let top = y < f64::from(output.y) + reach;
        let bottom = y >= f64::from(output.bottom()) - reach;
        match (left, right, top, bottom) {
            (true, _, true, _) => Some(Corner::TopLeft),
            (_, true, true, _) => Some(Corner::TopRight),
            (true, _, _, true) => Some(Corner::BottomLeft),
            (_, true, _, true) => Some(Corner::BottomRight),
            _ => None,
        }
    }
}

/// Validated corner actions.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HotCorners {
    pub top_left: Option<Action>,
    pub top_right: Option<Action>,
    pub bottom_left: Option<Action>,
    pub bottom_right: Option<Action>,
}

impl HotCorners {
    pub fn action(&self, corner: Corner) -> Option<&Action> {
        match corner {
            Corner::TopLeft => self.top_left.as_ref(),
            Corner::TopRight => self.top_right.as_ref(),
            Corner::BottomLeft => self.bottom_left.as_ref(),
            Corner::BottomRight => self.bottom_right.as_ref(),
        }
    }

    pub fn is_empty(&self) -> bool {
        Corner::ALL
            .into_iter()
            .all(|corner| self.action(corner).is_none())
    }
}

/// `[hot_corners]` as written in the configuration file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RawHotCorners {
    pub top_left: Option<String>,
    pub top_right: Option<String>,
    pub bottom_left: Option<String>,
    pub bottom_right: Option<String>,
}

impl RawHotCorners {
    /// Parse every action in the action grammar used by keybindings.
    pub fn validate(&self) -> Result<HotCorners, ConfigError> {
        let parse = |action: &Option<String>| -> Result<Option<Action>, ConfigError> {
            action
                .as_deref()
                .map(Action::parse)
                .transpose()
                .map_err(ConfigError::from)
        };
        Ok(HotCorners {
            top_left: parse(&self.top_left)?,
            top_right: parse(&self.top_right)?,
            bottom_left: parse(&self.bottom_left)?,
            bottom_right: parse(&self.bottom_right)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_are_found_within_reach_only() {
        let output = Rect::new(0, 0, 1280, 800);
        assert_eq!(Corner::at(output, 0.0, 0.0, 2), Some(Corner::TopLeft));
        assert_eq!(Corner::at(output, 1279.5, 1.0, 2), Some(Corner::TopRight));
        assert_eq!(Corner::at(output, 1.0, 799.0, 2), Some(Corner::BottomLeft));
        assert_eq!(
            Corner::at(output, 1279.0, 799.0, 2),
            Some(Corner::BottomRight)
        );
        assert_eq!(
            Corner::at(output, 0.0, 400.0, 2),
            None,
            "an edge is not a corner"
        );
        assert_eq!(Corner::at(output, 640.0, 400.0, 24), None);
    }

    #[test]
    fn corner_actions_use_the_binding_grammar() {
        let raw = RawHotCorners {
            top_left: Some("focus-cycle next".into()),
            ..RawHotCorners::default()
        };
        let corners = raw.validate().expect("valid");
        assert_eq!(
            corners.action(Corner::TopLeft),
            Some(&Action::FocusCycle { delta: 1 })
        );
        assert!(corners.action(Corner::BottomRight).is_none());
        assert!(!corners.is_empty());
        assert!(HotCorners::default().is_empty());

        let broken = RawHotCorners {
            bottom_right: Some("not-an-action".into()),
            ..RawHotCorners::default()
        };
        assert!(broken.validate().is_err());
    }
}
