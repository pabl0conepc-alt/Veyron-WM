//! Integer geometry primitives.
//!
//! Veyron keeps all window management math in integer logical pixels. Floating
//! point is only used while accumulating column offsets, so that rounding never
//! depends on the order in which columns are visited.

use serde::{Deserialize, Serialize};

/// A point in logical compositor coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const ZERO: Point = Point { x: 0, y: 0 };

    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

impl From<(i32, i32)> for Point {
    fn from((x, y): (i32, i32)) -> Self {
        Self::new(x, y)
    }
}

/// A width/height pair in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Size {
    pub w: i32,
    pub h: i32,
}

impl Size {
    pub const ZERO: Size = Size { w: 0, h: 0 };

    pub const fn new(w: i32, h: i32) -> Self {
        Self { w, h }
    }

    pub fn is_empty(self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    /// Clamp both dimensions into `0..=max`, component-wise.
    pub fn clamp_to(self, max: Size) -> Size {
        Size::new(self.w.min(max.w).max(0), self.h.min(max.h).max(0))
    }
}

impl From<(i32, i32)> for Size {
    fn from((w, h): (i32, i32)) -> Self {
        Self::new(w, h)
    }
}

/// An axis-aligned rectangle in logical compositor coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const ZERO: Rect = Rect {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    };

    /// Create a rectangle. Negative extents are clamped to zero so that a
    /// `Rect` is always a valid (possibly empty) area.
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self {
            x,
            y,
            w: w.max(0),
            h: h.max(0),
        }
    }

    pub fn from_size(size: Size) -> Self {
        Self::new(0, 0, size.w, size.h)
    }

    pub fn size(&self) -> Size {
        Size::new(self.w, self.h)
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    pub fn right(&self) -> i32 {
        self.x.saturating_add(self.w)
    }

    pub fn bottom(&self) -> i32 {
        self.y.saturating_add(self.h)
    }

    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.x && point.y >= self.y && point.x < self.right() && point.y < self.bottom()
    }

    pub fn intersects(&self, other: Rect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    pub fn intersection(&self, other: Rect) -> Option<Rect> {
        if !self.intersects(other) {
            return None;
        }
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        Some(Rect::new(x, y, right - x, bottom - y))
    }

    /// Shrink the rectangle by `amount` on every side. Never produces a
    /// negative extent: a rectangle that is too small becomes empty.
    pub fn inset(&self, amount: i32) -> Rect {
        let amount = amount.max(0);
        Rect::new(
            self.x + amount,
            self.y + amount,
            self.w - amount * 2,
            self.h - amount * 2,
        )
    }

    pub fn shift(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    /// Center a rectangle of `size` inside this one, clamping it to fit.
    pub fn centered(&self, size: Size) -> Rect {
        let size = size.clamp_to(self.size());
        Rect::new(
            self.x + (self.w - size.w) / 2,
            self.y + (self.h - size.h) / 2,
            size.w,
            size.h,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_extents_are_clamped() {
        let rect = Rect::new(0, 0, -40, -2);
        assert_eq!(rect.w, 0);
        assert_eq!(rect.h, 0);
        assert!(rect.is_empty());
    }

    #[test]
    fn inset_can_empty_a_rectangle() {
        let rect = Rect::new(10, 10, 20, 20).inset(15);
        assert!(rect.is_empty());
    }

    #[test]
    fn intersection_of_overlapping_rects() {
        let a = Rect::new(0, 0, 100, 100);
        let b = Rect::new(50, 20, 100, 40);
        assert_eq!(a.intersection(b), Some(Rect::new(50, 20, 50, 40)));
    }

    #[test]
    fn intersection_of_disjoint_rects_is_none() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(10, 0, 10, 10);
        assert_eq!(a.intersection(b), None);
        assert!(!a.intersects(b));
    }

    #[test]
    fn contains_uses_half_open_bounds() {
        let rect = Rect::new(0, 0, 10, 10);
        assert!(rect.contains(Point::new(0, 0)));
        assert!(rect.contains(Point::new(9, 9)));
        assert!(!rect.contains(Point::new(10, 9)));
    }

    #[test]
    fn centered_clamps_to_container() {
        let container = Rect::new(0, 0, 100, 50);
        assert_eq!(
            container.centered(Size::new(200, 20)),
            Rect::new(0, 15, 100, 20)
        );
    }
}
