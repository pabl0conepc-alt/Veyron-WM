//! Veyron Snap — screen zones for floating windows — and picture-in-picture
//! geometry.
//!
//! Both features only ever reposition a *floating* window, so the tiling layouts
//! and the strip invariants are untouched. Everything here is pure geometry:
//! given a viewport, a gap and a zone, the answer is one rectangle.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{action::ActionParseError, geometry::Rect};

/// Width of a picture-in-picture window as a fraction of the viewport.
pub const DEFAULT_PIP_RATIO: f64 = 0.28;

/// Narrowest and widest picture-in-picture ratio accepted.
pub const MIN_PIP_RATIO: f64 = 0.1;
pub const MAX_PIP_RATIO: f64 = 0.5;

/// A region of the usable area a floating window can snap to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnapZone {
    /// Left half of the viewport.
    Left,
    /// Right half of the viewport.
    Right,
    /// Top half of the viewport.
    Top,
    /// Bottom half of the viewport.
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    /// Left third of the viewport.
    LeftThird,
    /// Middle third of the viewport.
    CenterThird,
    /// Right third of the viewport.
    RightThird,
    /// Centered window at 60% of the viewport.
    Center,
    /// The whole usable area.
    Maximize,
}

impl SnapZone {
    pub const ALL: [SnapZone; 13] = [
        SnapZone::Left,
        SnapZone::Right,
        SnapZone::Top,
        SnapZone::Bottom,
        SnapZone::TopLeft,
        SnapZone::TopRight,
        SnapZone::BottomLeft,
        SnapZone::BottomRight,
        SnapZone::LeftThird,
        SnapZone::CenterThird,
        SnapZone::RightThird,
        SnapZone::Center,
        SnapZone::Maximize,
    ];

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "left" | "l" | "west" => Some(SnapZone::Left),
            "right" | "r" | "east" => Some(SnapZone::Right),
            "top" | "up" | "north" => Some(SnapZone::Top),
            "bottom" | "down" | "south" => Some(SnapZone::Bottom),
            "top-left" | "left-top" | "tl" | "north-west" => Some(SnapZone::TopLeft),
            "top-right" | "right-top" | "tr" | "north-east" => Some(SnapZone::TopRight),
            "bottom-left" | "left-bottom" | "bl" | "south-west" => Some(SnapZone::BottomLeft),
            "bottom-right" | "right-bottom" | "br" | "south-east" => Some(SnapZone::BottomRight),
            "left-third" | "third-left" | "one-third" => Some(SnapZone::LeftThird),
            "center-third" | "middle-third" | "centre-third" => Some(SnapZone::CenterThird),
            "right-third" | "third-right" | "two-thirds" => Some(SnapZone::RightThird),
            "center" | "centre" | "middle" => Some(SnapZone::Center),
            "maximize" | "max" | "full" | "fullscreen-window" => Some(SnapZone::Maximize),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SnapZone::Left => "left",
            SnapZone::Right => "right",
            SnapZone::Top => "top",
            SnapZone::Bottom => "bottom",
            SnapZone::TopLeft => "top-left",
            SnapZone::TopRight => "top-right",
            SnapZone::BottomLeft => "bottom-left",
            SnapZone::BottomRight => "bottom-right",
            SnapZone::LeftThird => "left-third",
            SnapZone::CenterThird => "center-third",
            SnapZone::RightThird => "right-third",
            SnapZone::Center => "center",
            SnapZone::Maximize => "maximize",
        }
    }

    /// Geometry of this zone inside `viewport`, leaving `gap` pixels between
    /// neighbouring zones and at the viewport edge.
    ///
    /// Every returned rectangle is non-empty and inside the viewport, even for
    /// degenerate viewports, so a snap can never hide a window off screen.
    pub fn rect(self, viewport: Rect, gap: i32) -> Rect {
        if viewport.is_empty() {
            return Rect::ZERO;
        }
        let gap = gap.max(0);

        let rect = match self {
            SnapZone::Maximize => viewport,
            SnapZone::Center => {
                let w = (viewport.w * 3 / 5).max(1);
                let h = (viewport.h * 3 / 5).max(1);
                Rect::new(
                    viewport.x + (viewport.w - w) / 2,
                    viewport.y + (viewport.h - h) / 2,
                    w,
                    h,
                )
            }
            SnapZone::Left => {
                let columns = split::<2>(viewport.x, viewport.w, gap);
                Rect::new(columns[0].0, viewport.y, columns[0].1, viewport.h)
            }
            SnapZone::Right => {
                let columns = split::<2>(viewport.x, viewport.w, gap);
                Rect::new(columns[1].0, viewport.y, columns[1].1, viewport.h)
            }
            SnapZone::Top => {
                let rows = split::<2>(viewport.y, viewport.h, gap);
                Rect::new(viewport.x, rows[0].0, viewport.w, rows[0].1)
            }
            SnapZone::Bottom => {
                let rows = split::<2>(viewport.y, viewport.h, gap);
                Rect::new(viewport.x, rows[1].0, viewport.w, rows[1].1)
            }
            SnapZone::TopLeft => quadrant(viewport, gap, 0, 0),
            SnapZone::TopRight => quadrant(viewport, gap, 1, 0),
            SnapZone::BottomLeft => quadrant(viewport, gap, 0, 1),
            SnapZone::BottomRight => quadrant(viewport, gap, 1, 1),
            SnapZone::LeftThird => {
                let columns = split::<3>(viewport.x, viewport.w, gap);
                Rect::new(columns[0].0, viewport.y, columns[0].1, viewport.h)
            }
            SnapZone::CenterThird => {
                let columns = split::<3>(viewport.x, viewport.w, gap);
                Rect::new(columns[1].0, viewport.y, columns[1].1, viewport.h)
            }
            SnapZone::RightThird => {
                let columns = split::<3>(viewport.x, viewport.w, gap);
                Rect::new(columns[2].0, viewport.y, columns[2].1, viewport.h)
            }
        };
        fit(viewport, rect)
    }
}

/// Clamp a zone rectangle into the viewport. A viewport too small to hold the
/// zone at all degrades to the whole viewport, so a snap never hides a window.
fn fit(viewport: Rect, rect: Rect) -> Rect {
    if rect.is_empty() {
        return viewport;
    }
    let w = rect.w.min(viewport.w);
    let h = rect.h.min(viewport.h);
    let x = rect
        .x
        .clamp(viewport.x, (viewport.right() - w).max(viewport.x));
    let y = rect
        .y
        .clamp(viewport.y, (viewport.bottom() - h).max(viewport.y));
    Rect::new(x, y, w, h)
}

impl fmt::Display for SnapZone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SnapZone {
    type Err = ActionParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        SnapZone::parse(value).ok_or_else(|| {
            ActionParseError(format!(
                "unknown snap zone '{value}' (expected left, right, top-left, left-third, maximize, ...)"
            ))
        })
    }
}

/// Corner a picture-in-picture window is docked to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PipCorner {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}

impl PipCorner {
    pub const ALL: [PipCorner; 4] = [
        PipCorner::TopLeft,
        PipCorner::TopRight,
        PipCorner::BottomLeft,
        PipCorner::BottomRight,
    ];

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "top-left" | "left-top" | "tl" => Some(PipCorner::TopLeft),
            "top-right" | "right-top" | "tr" => Some(PipCorner::TopRight),
            "bottom-left" | "left-bottom" | "bl" => Some(PipCorner::BottomLeft),
            "bottom-right" | "right-bottom" | "br" | "default" => Some(PipCorner::BottomRight),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PipCorner::TopLeft => "top-left",
            PipCorner::TopRight => "top-right",
            PipCorner::BottomLeft => "bottom-left",
            PipCorner::BottomRight => "bottom-right",
        }
    }

    /// Geometry for a picture-in-picture window docked to this corner.
    ///
    /// The window keeps a 16:9 shape at `ratio` of the viewport width, clamped
    /// into the viewport, so it always stays fully visible and never covers more
    /// than half of the screen.
    pub fn rect(self, viewport: Rect, ratio: f64, gap: i32) -> Rect {
        if viewport.is_empty() {
            return Rect::ZERO;
        }
        let ratio = if ratio.is_finite() && ratio > 0.0 {
            ratio.clamp(MIN_PIP_RATIO, MAX_PIP_RATIO)
        } else {
            DEFAULT_PIP_RATIO
        };
        let width = ((viewport.w as f64 * ratio).round() as i32).clamp(1, viewport.w);
        let height = ((width as f64 * 9.0 / 16.0).round() as i32).clamp(1, viewport.h);
        let gap = gap.max(0);
        let max_x = (viewport.right() - width).max(viewport.x);
        let max_y = (viewport.bottom() - height).max(viewport.y);
        let x = match self {
            PipCorner::TopLeft | PipCorner::BottomLeft => viewport.x + gap,
            PipCorner::TopRight | PipCorner::BottomRight => viewport.right() - width - gap,
        };
        let y = match self {
            PipCorner::TopLeft | PipCorner::TopRight => viewport.y + gap,
            PipCorner::BottomLeft | PipCorner::BottomRight => viewport.bottom() - height - gap,
        };
        Rect::new(
            x.clamp(viewport.x, max_x),
            y.clamp(viewport.y, max_y),
            width,
            height,
        )
    }
}

impl fmt::Display for PipCorner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PipCorner {
    type Err = ActionParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        PipCorner::parse(value).ok_or_else(|| {
            ActionParseError(format!(
                "unknown picture-in-picture corner '{value}' (expected top-left, top-right, bottom-left or bottom-right)"
            ))
        })
    }
}

/// Split an axis into `N` slots, leaving `gap` pixels between them.
///
/// The last slot absorbs the rounding remainder so the slots always cover the
/// whole extent exactly.
fn split<const N: usize>(origin: i32, extent: i32, gap: i32) -> [(i32, i32); N] {
    let gap = gap.max(0);
    let parts = N as i32;
    let usable = (extent - gap * (parts - 1)).max(parts);
    let base = usable / parts;
    let mut slots = [(0, 0); N];
    let mut cursor = origin;
    for (index, slot) in slots.iter_mut().enumerate() {
        let size = if index + 1 == N {
            usable - base * (parts - 1)
        } else {
            base
        };
        *slot = (cursor, size);
        cursor += size + gap;
    }
    slots
}

/// One of the four quadrants of the viewport.
fn quadrant(viewport: Rect, gap: i32, column: usize, row: usize) -> Rect {
    let columns = split::<2>(viewport.x, viewport.w, gap);
    let rows = split::<2>(viewport.y, viewport.h, gap);
    Rect::new(
        columns[column].0,
        rows[row].0,
        columns[column].1,
        rows[row].1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewport() -> Rect {
        Rect::new(0, 0, 1000, 600)
    }

    #[test]
    fn every_zone_round_trips_through_text() {
        for zone in SnapZone::ALL {
            assert_eq!(SnapZone::parse(zone.as_str()), Some(zone));
            assert_eq!(zone.to_string(), zone.as_str());
        }
        for corner in PipCorner::ALL {
            assert_eq!(PipCorner::parse(corner.as_str()), Some(corner));
            assert_eq!(corner.to_string(), corner.as_str());
        }
        assert_eq!(SnapZone::parse("Top_Left"), Some(SnapZone::TopLeft));
        assert_eq!(SnapZone::parse("nope"), None);
        assert!(PipCorner::parse("nope").is_none());
    }

    #[test]
    fn halves_fill_the_viewport_without_overlapping() {
        let halves = [
            SnapZone::Left.rect(viewport(), 8),
            SnapZone::Right.rect(viewport(), 8),
        ];
        assert_eq!(halves[0], Rect::new(0, 0, 496, 600));
        assert_eq!(halves[1], Rect::new(504, 0, 496, 600));
        assert_eq!(halves[0].intersection(halves[1]), None);

        let quarters = [
            SnapZone::TopLeft.rect(viewport(), 8),
            SnapZone::TopRight.rect(viewport(), 8),
            SnapZone::BottomLeft.rect(viewport(), 8),
            SnapZone::BottomRight.rect(viewport(), 8),
        ];
        assert_eq!(quarters[0], Rect::new(0, 0, 496, 296));
        assert_eq!(quarters[3], Rect::new(504, 304, 496, 296));
        for (index, rect) in quarters.iter().enumerate() {
            assert!(viewport().intersection(*rect) == Some(*rect));
            for other in &quarters[index + 1..] {
                assert_eq!(rect.intersection(*other), None, "quadrants never overlap");
            }
        }
    }

    #[test]
    fn thirds_cover_the_whole_width() {
        let thirds = [
            SnapZone::LeftThird.rect(viewport(), 6),
            SnapZone::CenterThird.rect(viewport(), 6),
            SnapZone::RightThird.rect(viewport(), 6),
        ];
        assert_eq!(thirds[0].x, 0);
        assert_eq!(thirds[2].right(), 1000);
        for pair in thirds.windows(2) {
            assert_eq!(pair[0].right() + 6, pair[1].x, "one gap between thirds");
        }
        for rect in thirds {
            assert_eq!(rect.h, 600);
            assert!(rect.w > 0);
        }
    }

    #[test]
    fn maximize_and_center_stay_inside_the_viewport() {
        let muted = Rect::new(100, 50, 800, 500);
        assert_eq!(SnapZone::Maximize.rect(muted, 12), muted);
        let center = SnapZone::Center.rect(muted, 12);
        assert_eq!(center, Rect::new(260, 150, 480, 300));
        assert_eq!(muted.intersection(center), Some(center));
    }

    #[test]
    fn tiny_viewports_still_produce_visible_geometry() {
        for zone in SnapZone::ALL {
            for (w, h) in [(1, 1), (2, 2), (7, 3), (3, 9)] {
                let viewport = Rect::new(0, 0, w, h);
                let rect = zone.rect(viewport, 24);
                assert!(!rect.is_empty(), "{zone} must not vanish on {w}x{h}");
                assert_eq!(viewport.intersection(rect), Some(rect));
            }
        }
    }

    #[test]
    fn picture_in_picture_docks_into_every_corner() {
        let viewport = Rect::new(0, 0, 1000, 600);
        let bottom_right = PipCorner::BottomRight.rect(viewport, 0.25, 12);
        assert_eq!(bottom_right.w, 250);
        assert_eq!(bottom_right.h, 141);
        assert_eq!(bottom_right.right(), 988);
        assert_eq!(bottom_right.bottom(), 588);

        let top_left = PipCorner::TopLeft.rect(viewport, 0.25, 12);
        assert_eq!(top_left, Rect::new(12, 12, 250, 141));

        for corner in PipCorner::ALL {
            let rect = corner.rect(viewport, 0.25, 12);
            assert_eq!(viewport.intersection(rect), Some(rect));
            assert!(rect.w <= viewport.w / 2);
        }
    }

    #[test]
    fn picture_in_picture_ratio_is_clamped() {
        let viewport = Rect::new(0, 0, 1000, 600);
        assert_eq!(
            PipCorner::BottomRight.rect(viewport, 5.0, 0).w,
            (MAX_PIP_RATIO * 1000.0) as i32
        );
        assert_eq!(
            PipCorner::BottomRight.rect(viewport, 0.0, 0).w,
            (DEFAULT_PIP_RATIO * 1000.0).round() as i32
        );
        assert_eq!(
            PipCorner::BottomRight.rect(viewport, f64::NAN, 0).w,
            (DEFAULT_PIP_RATIO * 1000.0).round() as i32
        );
    }

    #[test]
    fn picture_in_picture_never_overflows_a_short_viewport() {
        let viewport = Rect::new(0, 0, 400, 60);
        for corner in PipCorner::ALL {
            let rect = corner.rect(viewport, 0.5, 8);
            assert_eq!(viewport.intersection(rect), Some(rect));
            assert!(rect.h <= viewport.h);
        }
    }
}
