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

/// The edges of a rectangle an interactive resize moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ResizeEdges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

impl ResizeEdges {
    /// Edges nearest to `point`: the rectangle is split in thirds on each axis,
    /// so grabbing near a corner resizes both axes and the middle of a side
    /// resizes only that side.
    pub fn nearest(rect: Rect, point: Point) -> Self {
        let third_w = rect.w / 3;
        let third_h = rect.h / 3;
        let mut edges = Self {
            left: point.x < rect.x + third_w,
            right: point.x >= rect.right() - third_w,
            top: point.y < rect.y + third_h,
            bottom: point.y >= rect.bottom() - third_h,
        };
        if !(edges.left || edges.right || edges.top || edges.bottom) {
            // The very centre: grow towards the bottom-right corner.
            edges.right = true;
            edges.bottom = true;
        }
        edges
    }

    pub fn is_empty(&self) -> bool {
        !(self.left || self.right || self.top || self.bottom)
    }
}

impl Rect {
    /// Resize by a pointer delta along `edges`, keeping the opposite edges
    /// in place and never shrinking below `min`.
    pub fn resized(&self, edges: ResizeEdges, dx: i32, dy: i32, min: Size) -> Rect {
        let (mut x, mut w) = (self.x, self.w);
        if edges.right {
            w = self.w.saturating_add(dx).max(min.w);
        } else if edges.left {
            w = self.w.saturating_sub(dx).max(min.w);
            x = self.right() - w;
        }
        let (mut y, mut h) = (self.y, self.h);
        if edges.bottom {
            h = self.h.saturating_add(dy).max(min.h);
        } else if edges.top {
            h = self.h.saturating_sub(dy).max(min.h);
            y = self.bottom() - h;
        }
        Rect::new(x, y, w, h)
    }
}

/// Space reserved along each edge of a rectangle, such as the exclusive zone
/// a panel claims at the top of an output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Insets {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

impl Insets {
    pub const ZERO: Insets = Insets {
        top: 0,
        right: 0,
        bottom: 0,
        left: 0,
    };

    /// The edges `inner` leaves free inside `outer`. Parts of `inner` outside
    /// `outer` reserve nothing, so every inset is at least zero.
    pub fn between(outer: Rect, inner: Rect) -> Insets {
        Insets {
            top: (inner.y - outer.y).max(0),
            right: (outer.right() - inner.right()).max(0),
            bottom: (outer.bottom() - inner.bottom()).max(0),
            left: (inner.x - outer.x).max(0),
        }
    }

    /// Shrink `rect` by these insets. A reservation that would leave nothing
    /// is ignored, so the result is never empty for a non-empty `rect`.
    pub fn shrink(&self, rect: Rect) -> Rect {
        let inner = Rect::new(
            rect.x.saturating_add(self.left),
            rect.y.saturating_add(self.top),
            rect.w.saturating_sub(self.left).saturating_sub(self.right),
            rect.h.saturating_sub(self.top).saturating_sub(self.bottom),
        );
        if inner.is_empty() {
            rect
        } else {
            inner
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_edges_follow_the_thirds_of_the_rectangle() {
        let rect = Rect::new(0, 0, 300, 300);
        let corner = ResizeEdges::nearest(rect, Point::new(10, 290));
        assert!(corner.left && corner.bottom && !corner.right && !corner.top);
        let side = ResizeEdges::nearest(rect, Point::new(295, 150));
        assert!(side.right && !side.top && !side.bottom && !side.left);
        let centre = ResizeEdges::nearest(rect, Point::new(150, 150));
        assert!(centre.right && centre.bottom);
        assert!(!centre.is_empty());
    }

    #[test]
    fn resizing_keeps_the_opposite_edge_and_the_minimum_size() {
        let rect = Rect::new(100, 100, 200, 200);
        let min = Size::new(64, 48);
        let top_left = ResizeEdges {
            left: true,
            top: true,
            ..ResizeEdges::default()
        };
        let grown = rect.resized(top_left, -50, -20, min);
        assert_eq!(grown, Rect::new(50, 80, 250, 220));
        assert_eq!(grown.right(), rect.right());

        let shrunk = rect.resized(top_left, 1000, 1000, min);
        assert_eq!(shrunk.size(), min);
        assert_eq!(
            (shrunk.right(), shrunk.bottom()),
            (rect.right(), rect.bottom())
        );

        let bottom_right = ResizeEdges {
            right: true,
            bottom: true,
            ..ResizeEdges::default()
        };
        assert_eq!(
            rect.resized(bottom_right, 30, -10, min),
            Rect::new(100, 100, 230, 190)
        );
        assert_eq!(rect.resized(ResizeEdges::default(), 30, 30, min), rect);
    }

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

    #[test]
    fn insets_round_trip_through_an_inner_rectangle() {
        let output = Rect::new(100, 0, 1280, 800);
        // A 30 px bar on top and a 20 px dock on the left.
        let usable = Rect::new(120, 30, 1260, 770);
        let insets = Insets::between(output, usable);
        assert_eq!(
            insets,
            Insets {
                top: 30,
                right: 0,
                bottom: 0,
                left: 20,
            }
        );
        assert_eq!(insets.shrink(output), usable);
        // Insets stay attached to the edges when the rectangle is resized.
        assert_eq!(
            insets.shrink(Rect::new(100, 0, 1920, 1080)),
            Rect::new(120, 30, 1900, 1050)
        );
    }

    #[test]
    fn insets_never_empty_a_rectangle() {
        let rect = Rect::new(0, 0, 100, 50);
        let greedy = Insets {
            top: 40,
            bottom: 40,
            ..Insets::ZERO
        };
        assert_eq!(greedy.shrink(rect), rect);
        assert_eq!(
            Insets::between(rect, Rect::new(-10, -10, 200, 200)),
            Insets::ZERO
        );
    }
}
