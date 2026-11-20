//! Animation primitives for window transitions.
//!
//! The compositor renders whatever geometry the window manager produces. To
//! animate between two layouts it needs an interpolator that is deterministic,
//! cheap and independent from a wall clock, so this module provides:
//!
//! - [`Spring`]: a damped harmonic oscillator for motion that carries velocity
//!   across retargets (opening, closing, retiling).
//! - [`Easing`]: closed-form curves for time-based transitions.
//! - [`Animator`] / [`RectAnimator`]: drive a value or a window rectangle from
//!   its current state to a target, given a `dt` supplied by the render loop.
//!
//! Nothing here starts a thread, reads the time or allocates per frame: the
//! caller owns the clock and the frame budget.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{geometry::Rect, window::WindowId, wm::PlacementInfo};

/// Spring parameters for motion with inertia.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpringConfig {
    /// How strongly the spring pulls towards its target.
    pub stiffness: f64,
    /// How quickly motion is bled off. `2 * sqrt(stiffness * mass)` is critical
    /// damping; higher values never overshoot.
    pub damping: f64,
    /// Inertia of the animated value.
    pub mass: f64,
}

impl Default for SpringConfig {
    fn default() -> Self {
        // Slightly under-damped so transitions have a little life, while still
        // settling in well under a frame budget's worth of seconds.
        Self {
            stiffness: 220.0,
            damping: 26.0,
            mass: 1.0,
        }
    }
}

impl SpringConfig {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("motion.spring.stiffness", self.stiffness),
            ("motion.spring.damping", self.damping),
            ("motion.spring.mass", self.mass),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(format!("{name} must be a positive number (got {value})"));
            }
        }
        Ok(())
    }

    /// Damping ratio: `< 1` overshoots, `1` is critical, `> 1` is sluggish.
    pub fn damping_ratio(&self) -> f64 {
        self.damping / (2.0 * (self.stiffness * self.mass).sqrt())
    }
}

/// A one-dimensional spring state.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Spring {
    position: f64,
    velocity: f64,
}

impl Spring {
    pub fn new(position: f64) -> Self {
        Self {
            position,
            velocity: 0.0,
        }
    }

    pub fn position(&self) -> f64 {
        self.position
    }

    pub fn velocity(&self) -> f64 {
        self.velocity
    }

    pub fn set_position(&mut self, position: f64) {
        self.position = position;
    }

    /// Advance towards `target` by `dt` seconds.
    ///
    /// Integrates with fixed sub-steps so a long frame (a stall, a debugger
    /// pause) cannot make the spring explode. Returns the new position.
    pub fn step(&mut self, target: f64, dt: f64, config: &SpringConfig) -> f64 {
        if !dt.is_finite() || dt <= 0.0 {
            return self.position;
        }
        let dt = dt.min(0.25);
        // 240 Hz sub-steps: five per 60 Hz frame, always stable at these ranges.
        let sub_steps = ((dt / (1.0 / 240.0)).ceil() as usize).clamp(1, 64);
        let step = dt / sub_steps as f64;
        for _ in 0..sub_steps {
            let acceleration = (-config.stiffness * (self.position - target)
                - config.damping * self.velocity)
                / config.mass;
            self.velocity += acceleration * step;
            self.position += self.velocity * step;
        }
        self.position
    }

    /// Whether the spring is close enough to `target` to stop animating.
    pub fn is_settled(&self, target: f64, config: &SpringConfig) -> bool {
        (self.position - target).abs() < 0.5 && self.velocity.abs() < 10.0 * config.mass
    }
}

/// Closed-form easing curves for time-based transitions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Easing {
    Linear,
    #[default]
    EaseOutCubic,
    EaseInOutCubic,
}

impl Easing {
    /// Sample the curve at `t`, clamped into `0..=1`.
    pub fn sample(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => t,
            Easing::EaseOutCubic => 1.0 - (1.0 - t).powi(3),
            Easing::EaseInOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
        }
    }
}

/// Animation policy from the configuration file.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MotionConfig {
    /// Master switch. Off by default: a compositor must be static and correct
    /// before it is pretty.
    pub enabled: bool,
    /// Duration of a time-based transition.
    pub duration_ms: u32,
    /// Honour the user's reduced-motion preference by snapping instantly.
    pub reduce_motion: bool,
    pub spring: SpringConfig,
}

impl Default for MotionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            duration_ms: 180,
            reduce_motion: false,
            spring: SpringConfig::default(),
        }
    }
}

impl MotionConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.duration_ms > 5000 {
            return Err(format!(
                "motion.duration_ms must be <= 5000 (got {})",
                self.duration_ms
            ));
        }
        self.spring.validate()
    }

    /// Whether transitions should actually interpolate.
    pub fn active(&self) -> bool {
        self.enabled && !self.reduce_motion && self.duration_ms > 0
    }

    pub fn duration_secs(&self) -> f64 {
        self.duration_ms as f64 / 1000.0
    }
}

/// A single animated scalar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Animator {
    spring: Spring,
    target: f64,
    config: SpringConfig,
    running: bool,
}

impl Animator {
    pub fn new(value: f64, config: SpringConfig) -> Self {
        Self {
            spring: Spring::new(value),
            target: value,
            config,
            running: false,
        }
    }

    pub fn value(&self) -> f64 {
        self.spring.position()
    }

    pub fn target(&self) -> f64 {
        self.target
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Aim at a new value, keeping the current velocity for continuity.
    pub fn retarget(&mut self, target: f64) {
        if target != self.target {
            self.target = target;
            self.running = true;
        }
    }

    /// Snap immediately, cancelling any motion.
    pub fn snap(&mut self, value: f64) {
        self.spring.set_position(value);
        self.target = value;
        self.running = false;
    }

    /// Advance by `dt` seconds and return the new value.
    pub fn advance(&mut self, dt: f64) -> f64 {
        if !self.running {
            return self.value();
        }
        self.spring.step(self.target, dt, &self.config);
        if self.spring.is_settled(self.target, &self.config) {
            self.spring.set_position(self.target);
            self.running = false;
        }
        self.value()
    }
}

/// Animated rectangle used for window geometry transitions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectAnimator {
    from: Rect,
    to: Rect,
    elapsed: f64,
    duration: f64,
    easing: Easing,
    running: bool,
}

impl RectAnimator {
    /// Create an animator from `from` to `to` under the given policy.
    ///
    /// When motion is disabled (or reduced) the rectangle starts at its target,
    /// which keeps every caller free of special cases.
    pub fn new(from: Rect, to: Rect, config: &MotionConfig) -> Self {
        let running = config.active() && from != to;
        Self {
            from: if running { from } else { to },
            to,
            elapsed: if running { 0.0 } else { config.duration_secs() },
            duration: config.duration_secs(),
            easing: Easing::EaseOutCubic,
            running,
        }
    }

    pub fn with_easing(mut self, easing: Easing) -> Self {
        self.easing = easing;
        self
    }

    pub fn from(&self) -> Rect {
        self.from
    }

    pub fn target(&self) -> Rect {
        self.to
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Redirect the animation to a new target without snapping.
    pub fn retarget(&mut self, to: Rect) {
        if to == self.to {
            return;
        }
        self.from = self.current();
        self.to = to;
        self.elapsed = 0.0;
    }

    /// Progress in `0..=1`.
    pub fn progress(&self) -> f64 {
        if self.duration <= 0.0 {
            return 1.0;
        }
        (self.elapsed / self.duration).clamp(0.0, 1.0)
    }

    /// The rectangle at the current progress.
    pub fn current(&self) -> Rect {
        if !self.running {
            return self.to;
        }
        let t = self.easing.sample(self.progress());
        interpolate_rect(self.from, self.to, t)
    }

    /// Advance by `dt` seconds and return the rectangle to draw.
    pub fn advance(&mut self, dt: f64) -> Rect {
        if !self.running {
            return self.to;
        }
        if dt.is_finite() && dt > 0.0 {
            self.elapsed += dt;
        }
        if self.elapsed >= self.duration {
            self.running = false;
            return self.to;
        }
        self.current()
    }
}

/// Per-window rectangle transitions, driven by the compositor's frame clock.
///
/// The window manager only produces target geometry; this type turns the gap
/// between the previous frame and the new target into an animation, and tells
/// the renderer whether it still needs another frame. It never reads a clock:
/// the caller supplies `dt`, so a slow or missed frame degrades gracefully
/// instead of changing the animation's speed.
#[derive(Debug, Clone)]
pub struct Transitions {
    config: MotionConfig,
    animators: HashMap<WindowId, RectAnimator>,
    /// Geometry each window was last drawn at, so the first move of a settled
    /// window animates from where it really was.
    known: HashMap<WindowId, Rect>,
}

impl Transitions {
    pub fn new(config: MotionConfig) -> Self {
        Self {
            config,
            animators: HashMap::new(),
            known: HashMap::new(),
        }
    }

    pub fn config(&self) -> &MotionConfig {
        &self.config
    }

    /// Replace the motion policy. Turning motion off drops every running
    /// transition, so the next frame draws the real target geometry.
    pub fn set_config(&mut self, config: MotionConfig) {
        self.config = config;
        if !self.config.active() {
            self.animators.clear();
        }
    }

    pub fn is_animating(&self) -> bool {
        !self.animators.is_empty()
    }

    pub fn len(&self) -> usize {
        self.animators.len()
    }

    pub fn is_empty(&self) -> bool {
        self.animators.is_empty()
    }

    pub fn clear(&mut self) {
        self.animators.clear();
        self.known.clear();
    }

    /// Animate a window from `current` to `target`. An existing transition is
    /// redirected from wherever it is now, so a retile never jumps back.
    pub fn animate(&mut self, id: WindowId, current: Rect, target: Rect) {
        if !self.config.active() {
            self.animators.remove(&id);
            return;
        }
        match self.animators.get_mut(&id) {
            Some(animator) => animator.retarget(target),
            None => {
                let animator = RectAnimator::new(current, target, &self.config);
                if animator.is_running() {
                    self.animators.insert(id, animator);
                }
            }
        }
        if self
            .animators
            .get(&id)
            .is_some_and(|animator| !animator.is_running())
        {
            self.animators.remove(&id);
        }
    }

    /// Follow a frame of placements: animate every window towards its new
    /// rectangle starting from its in-flight position, and forget windows that
    /// are no longer drawn.
    pub fn sync(&mut self, placements: &[PlacementInfo]) {
        for placement in placements {
            let current = self
                .animators
                .get(&placement.window)
                .map(RectAnimator::current)
                .or_else(|| self.known.get(&placement.window).copied())
                .unwrap_or(placement.rect);
            self.animate(placement.window, current, placement.rect);
            self.known.insert(placement.window, placement.rect);
        }
        self.known
            .retain(|id, _| placements.iter().any(|placement| placement.window == *id));
        if !self.animators.is_empty() {
            self.animators
                .retain(|id, _| placements.iter().any(|placement| placement.window == *id));
        }
    }

    /// Forget a window (closed, or parked out of the frame).
    pub fn forget(&mut self, id: WindowId) {
        self.animators.remove(&id);
        self.known.remove(&id);
    }

    /// Advance every running transition by `dt` seconds. Returns whether any
    /// animation is still running, which is exactly "ask for another frame".
    pub fn tick(&mut self, dt: f64) -> bool {
        if self.animators.is_empty() {
            return false;
        }
        self.animators.retain(|_, animator| {
            animator.advance(dt);
            animator.is_running()
        });
        !self.animators.is_empty()
    }

    /// Rectangle to draw for a window, when it is mid-transition.
    pub fn rect(&self, id: WindowId) -> Option<Rect> {
        self.animators.get(&id).map(RectAnimator::current)
    }

    /// Every in-flight window and the rectangle to draw for it.
    pub fn rects(&self) -> impl Iterator<Item = (WindowId, Rect)> + '_ {
        self.animators
            .iter()
            .map(|(id, animator)| (*id, animator.current()))
    }
}

fn interpolate_rect(from: Rect, to: Rect, t: f64) -> Rect {
    let lerp = |a: i32, b: i32| -> i32 {
        let value = a as f64 + (b as f64 - a as f64) * t;
        value.round() as i32
    };
    Rect::new(
        lerp(from.x, to.x),
        lerp(from.y, to.y),
        lerp(from.w, to.w),
        lerp(from.h, to.h),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_curves_hit_both_endpoints_and_never_leave_the_unit_interval() {
        for easing in [Easing::Linear, Easing::EaseOutCubic, Easing::EaseInOutCubic] {
            assert_eq!(easing.sample(0.0), 0.0);
            assert_eq!(easing.sample(1.0), 1.0);
            assert_eq!(easing.sample(-5.0), 0.0);
            assert_eq!(easing.sample(5.0), 1.0);
            let mut previous = 0.0;
            for step in 0..=100 {
                let value = easing.sample(step as f64 / 100.0);
                assert!((0.0..=1.0).contains(&value));
                assert!(value >= previous - 1e-9, "curves are monotonic");
                previous = value;
            }
        }
    }

    #[test]
    fn spring_converges_to_its_target() {
        let config = SpringConfig::default();
        assert!(config.damping_ratio() < 1.0, "default has some life");
        let mut spring = Spring::new(0.0);
        for _ in 0..600 {
            spring.step(100.0, 1.0 / 240.0, &config);
        }
        assert!((spring.position() - 100.0).abs() < 0.5);
        assert!(spring.is_settled(100.0, &config));
    }

    #[test]
    fn spring_survives_huge_frame_deltas() {
        let config = SpringConfig::default();
        let mut spring = Spring::new(0.0);
        spring.step(500.0, 10.0, &config);
        assert!(spring.position().is_finite());
        assert!(spring.position() <= 500.0 + 1.0);
        spring.step(0.0, f64::NAN, &config);
        assert!(spring.position().is_finite());
    }

    #[test]
    fn animator_retarget_keeps_velocity_and_snaps_cleanly() {
        let config = SpringConfig::default();
        let mut animator = Animator::new(0.0, config);
        animator.retarget(120.0);
        animator.advance(0.05);
        assert!(animator.is_running());
        assert!(animator.value() > 0.0);

        animator.retarget(400.0);
        assert_eq!(animator.target(), 400.0);
        for _ in 0..600 {
            animator.advance(1.0 / 240.0);
        }
        assert_eq!(animator.value(), 400.0);
        assert!(!animator.is_running());

        animator.snap(12.0);
        assert_eq!(animator.value(), 12.0);
        assert_eq!(animator.advance(0.2), 12.0);
    }

    #[test]
    fn rect_animation_interpolates_and_finishes_exactly() {
        let config = MotionConfig {
            enabled: true,
            duration_ms: 200,
            ..MotionConfig::default()
        };
        let from = Rect::new(0, 0, 100, 100);
        let to = Rect::new(200, 100, 300, 400);
        let mut animator = RectAnimator::new(from, to, &config);
        assert!(animator.is_running());

        let middle = animator.advance(0.1);
        assert!(middle.x > from.x && middle.x < to.x);
        assert!(middle.y > from.y && middle.y < to.y);
        assert!(middle.w > from.w && middle.w < to.w);
        assert!(middle.h > from.h && middle.h < to.h);
        assert_eq!(animator.advance(0.1), to);
        assert!(!animator.is_running());
        assert_eq!(animator.current(), to);
    }

    #[test]
    fn retargeting_a_rect_restarts_from_the_current_position() {
        let config = MotionConfig {
            enabled: true,
            duration_ms: 100,
            ..MotionConfig::default()
        };
        let mut animator =
            RectAnimator::new(Rect::new(0, 0, 10, 10), Rect::new(100, 0, 10, 10), &config);
        let current = animator.advance(0.05);
        animator.retarget(Rect::new(0, 500, 10, 10));
        assert_eq!(animator.from(), current);
        assert!(animator.is_running());
        assert_eq!(animator.advance(0.1), Rect::new(0, 500, 10, 10));
    }

    #[test]
    fn disabled_or_reduced_motion_snaps_to_the_target() {
        for config in [
            MotionConfig::default(),
            MotionConfig {
                enabled: true,
                reduce_motion: true,
                ..MotionConfig::default()
            },
            MotionConfig {
                enabled: true,
                duration_ms: 0,
                ..MotionConfig::default()
            },
        ] {
            assert!(!config.active());
            let animator = RectAnimator::new(Rect::ZERO, Rect::new(10, 10, 10, 10), &config);
            assert!(!animator.is_running());
            assert_eq!(animator.current(), Rect::new(10, 10, 10, 10));
        }
    }

    #[test]
    fn transitions_follow_a_frame_of_placements_and_settle() {
        let config = MotionConfig {
            enabled: true,
            duration_ms: 100,
            ..MotionConfig::default()
        };
        let mut transitions = Transitions::new(config);
        let id = WindowId(1);
        let placement = |rect: Rect| PlacementInfo {
            window: id,
            rect,
            floating: false,
            fullscreen: false,
            opacity: 1.0,
            blur: false,
            pip: false,
        };
        let start = Rect::new(0, 0, 100, 100);
        let end = Rect::new(400, 0, 100, 100);

        transitions.sync(&[placement(start)]);
        assert!(!transitions.is_animating(), "nothing to animate yet");

        transitions.sync(&[placement(end)]);
        assert!(transitions.is_animating());
        let middle = transitions.rect(id).expect("in flight");
        assert!(middle.x >= start.x && middle.x <= end.x);

        assert!(!transitions.tick(0.25), "the transition finished");
        assert_eq!(transitions.rect(id), None);
        assert!(transitions.is_empty());
    }

    #[test]
    fn transitions_snap_and_clear_when_motion_turns_off() {
        let config = MotionConfig {
            enabled: true,
            duration_ms: 100,
            ..MotionConfig::default()
        };
        let mut transitions = Transitions::new(config);
        let id = WindowId(2);
        transitions.animate(id, Rect::new(0, 0, 10, 10), Rect::new(50, 50, 10, 10));
        assert!(transitions.tick(0.05));

        transitions.set_config(MotionConfig {
            enabled: false,
            ..config
        });
        assert!(!transitions.is_animating());
        assert_eq!(transitions.rect(id), None);

        transitions.animate(id, Rect::ZERO, Rect::new(5, 5, 5, 5));
        assert!(transitions.is_empty(), "disabled motion never animates");
    }

    #[test]
    fn retargeting_and_forgetting_windows_keeps_the_set_small() {
        let config = MotionConfig {
            enabled: true,
            duration_ms: 200,
            ..MotionConfig::default()
        };
        let mut transitions = Transitions::new(config);
        let (a, b) = (WindowId(3), WindowId(4));
        transitions.animate(a, Rect::ZERO, Rect::new(100, 0, 50, 50));
        transitions.animate(b, Rect::ZERO, Rect::new(0, 100, 50, 50));
        assert_eq!(transitions.len(), 2);

        transitions.animate(a, Rect::ZERO, Rect::new(200, 0, 80, 50));
        assert_eq!(transitions.len(), 2, "retarget reuses the same track");

        transitions.forget(b);
        assert_eq!(transitions.rects().count(), 1);

        transitions.sync(&[]);
        assert!(transitions.is_empty(), "undrawn windows are dropped");
    }

    #[test]
    fn motion_validation_rejects_unusable_values() {
        assert!(MotionConfig::default().validate().is_ok());
        assert!(MotionConfig {
            duration_ms: 9000,
            ..MotionConfig::default()
        }
        .validate()
        .is_err());
        assert!(MotionConfig {
            spring: SpringConfig {
                stiffness: 0.0,
                ..SpringConfig::default()
            },
            ..MotionConfig::default()
        }
        .validate()
        .is_err());
        assert!(SpringConfig {
            mass: f64::INFINITY,
            ..SpringConfig::default()
        }
        .validate()
        .is_err());
    }
}
