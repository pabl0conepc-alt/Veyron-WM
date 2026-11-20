//! Touchpad gesture policy.
//!
//! Wayland delivers libinput gesture events as deltas; deciding when a swipe is
//! a workspace change is policy, not protocol. This module accumulates those
//! deltas, resolves the dominant axis and emits a [`Direction`] as soon as a
//! swipe crosses the configured threshold, keeping the remainder so a long
//! swipe can move through several workspaces without lifting the fingers.
//!
//! It is deliberately tiny and allocation-free: the input layer calls `update`
//! per event and cancels on gesture end or interruption.

use serde::{Deserialize, Serialize};

use crate::action::Direction;

/// Gesture thresholds from the configuration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GestureConfig {
    /// Enable gesture-driven workspace changes.
    pub enabled: bool,
    /// Distance in logical pixels a swipe must cover to count.
    pub swipe_threshold: f64,
    /// How much larger the dominant axis must be to win (`1.0` disables the
    /// check, higher values require a straighter swipe).
    pub axis_ratio: f64,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            swipe_threshold: 220.0,
            axis_ratio: 1.2,
        }
    }
}

impl GestureConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !self.swipe_threshold.is_finite() || self.swipe_threshold <= 0.0 {
            return Err(format!(
                "gestures.swipe_threshold must be a positive number (got {})",
                self.swipe_threshold
            ));
        }
        if !self.axis_ratio.is_finite() || self.axis_ratio < 1.0 {
            return Err(format!(
                "gestures.axis_ratio must be >= 1.0 (got {})",
                self.axis_ratio
            ));
        }
        Ok(())
    }
}

/// Accumulates swipe deltas into workspace directions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwipeTracker {
    config: GestureConfig,
    dx: f64,
    dy: f64,
}

impl SwipeTracker {
    pub fn new(config: GestureConfig) -> Self {
        Self {
            config,
            dx: 0.0,
            dy: 0.0,
        }
    }

    pub fn config(&self) -> &GestureConfig {
        &self.config
    }

    pub fn set_config(&mut self, config: GestureConfig) {
        self.config = config;
        self.cancel();
    }

    /// Pixels accumulated on each axis since the last emitted direction.
    pub fn accumulated(&self) -> (f64, f64) {
        (self.dx, self.dy)
    }

    /// Feed one gesture delta. Returns a direction when the swipe crossed the
    /// threshold, and in that case removes the threshold distance from the
    /// dominant axis so the next threshold can be reached with the same swipe.
    pub fn update(&mut self, dx: f64, dy: f64) -> Option<Direction> {
        if !self.config.enabled || !dx.is_finite() || !dy.is_finite() {
            return None;
        }
        // Every event has to commit to one axis: diagonal noise below the ratio
        // is dropped instead of being accumulated on both axes, so a wobbly
        // swipe cannot add up to a false positive.
        let horizontal = dx.abs() >= dy.abs() * self.config.axis_ratio;
        let vertical = dy.abs() >= dx.abs() * self.config.axis_ratio;
        if !horizontal && !vertical {
            return None;
        }
        if horizontal {
            self.dx += dx;
            self.dy = 0.0;
        } else {
            self.dy += dy;
            self.dx = 0.0;
        }

        let (value, threshold) = if horizontal {
            (self.dx, self.config.swipe_threshold)
        } else {
            (self.dy, self.config.swipe_threshold)
        };
        if value.abs() < threshold {
            return None;
        }

        let direction = match (horizontal, value >= 0.0) {
            (true, true) => Direction::Right,
            (true, false) => Direction::Left,
            (false, true) => Direction::Down,
            (false, false) => Direction::Up,
        };
        if horizontal {
            self.dx -= threshold.copysign(value);
            self.dy = 0.0;
        } else {
            self.dy -= threshold.copysign(value);
            self.dx = 0.0;
        }
        Some(direction)
    }

    /// Forget the accumulated distance, for gesture end or interruption.
    pub fn cancel(&mut self) {
        self.dx = 0.0;
        self.dy = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker() -> SwipeTracker {
        SwipeTracker::new(GestureConfig {
            enabled: true,
            swipe_threshold: 100.0,
            axis_ratio: 1.5,
        })
    }

    #[test]
    fn a_swipe_emits_one_direction_per_threshold() {
        let mut tracker = tracker();
        assert_eq!(tracker.update(40.0, 0.0), None);
        assert_eq!(tracker.update(40.0, 0.0), None);
        assert_eq!(tracker.update(40.0, 0.0), Some(Direction::Right));
        assert_eq!(tracker.accumulated(), (20.0, 0.0), "remainder is kept");

        assert_eq!(tracker.update(-150.0, 0.0), Some(Direction::Left));
        assert_eq!(tracker.accumulated(), (-30.0, 0.0));
        assert_eq!(tracker.update(-80.0, 0.0), Some(Direction::Left));
        assert_eq!(
            tracker.update(-40.0, 0.0),
            None,
            "below the threshold again"
        );
    }

    #[test]
    fn the_dominant_axis_wins_and_diagonals_are_ignored() {
        let mut vertical = tracker();
        assert_eq!(vertical.update(30.0, 200.0), Some(Direction::Down));

        let mut diagonal = tracker();
        assert_eq!(diagonal.update(-90.0, 100.0), None, "diagonal noise");
        assert_eq!(diagonal.accumulated(), (0.0, 0.0), "noise is not stored");
        assert_eq!(diagonal.update(-120.0, 0.0), Some(Direction::Left));
        assert_eq!(diagonal.update(0.0, -120.0), Some(Direction::Up));
    }

    #[test]
    fn disabled_or_invalid_input_never_triggers() {
        let mut disabled = SwipeTracker::new(GestureConfig::default());
        assert!(!disabled.config().enabled);
        assert_eq!(disabled.update(10_000.0, 0.0), None);
        assert_eq!(disabled.accumulated(), (0.0, 0.0));

        let mut active = tracker();
        assert_eq!(active.update(f64::NAN, 0.0), None);
        assert_eq!(active.update(0.0, f64::INFINITY), None);
        active.update(50.0, 0.0);
        active.cancel();
        assert_eq!(active.accumulated(), (0.0, 0.0));
        assert_eq!(active.update(50.0, 0.0), None, "cancel drops the rest");
    }

    #[test]
    fn gesture_configuration_is_validated() {
        assert!(GestureConfig::default().validate().is_ok());
        assert!(GestureConfig {
            swipe_threshold: 0.0,
            ..GestureConfig::default()
        }
        .validate()
        .is_err());
        assert!(GestureConfig {
            axis_ratio: 0.5,
            ..GestureConfig::default()
        }
        .validate()
        .is_err());
    }
}
