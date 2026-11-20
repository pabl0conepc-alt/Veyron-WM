//! Tiling layouts beyond the scrolling strip.
//!
//! Veyron started as a scrollable tiling compositor and keeps that model as one
//! layout among several. This module owns the geometry of the additional
//! layouts so the compositor can switch between them without touching window
//! state:
//!
//! - [`LayoutKind::Scroll`] is the original endless horizontal strip; its
//!   geometry stays in [`crate::layout::ScrollLayout`], which also owns the
//!   column/focus order for every layout.
//! - [`LayoutKind::MasterStack`] gives one master window and a stack.
//! - [`LayoutKind::Dwindle`] splits the screen recursively (spiral).
//! - [`LayoutKind::Auto`] resolves per window count (dynamic tiling).
//!
//! Everything here is pure geometry: no I/O, no clock, no protocol types.

use serde::{Deserialize, Serialize};

use crate::{
    geometry::Rect,
    layout::{LayoutConfig, Placement},
    window::WindowId,
};

/// Fraction of the viewport given to the master window by default.
pub const DEFAULT_MASTER_RATIO: f64 = 0.6;

/// Which tiling algorithm a workspace uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayoutKind {
    /// The endless horizontal strip of columns (Veyron's original model).
    #[default]
    Scroll,
    /// One master window on the left, the rest stacked on the right.
    MasterStack,
    /// Recursive spiral splits, alternating between the two axes.
    Dwindle,
    /// Pick [`LayoutKind::MasterStack`] or [`LayoutKind::Dwindle`] from the
    /// number of tiled windows (dynamic tiling).
    Auto,
}

impl LayoutKind {
    /// Every layout a user can select explicitly.
    pub const SELECTABLE: [LayoutKind; 4] = [
        LayoutKind::Scroll,
        LayoutKind::MasterStack,
        LayoutKind::Dwindle,
        LayoutKind::Auto,
    ];

    /// Parse the canonical name, accepting the aliases people are used to.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "scroll" | "scrolling" | "strip" => Some(LayoutKind::Scroll),
            "master" | "master-stack" | "masterstack" | "ms" => Some(LayoutKind::MasterStack),
            "dwindle" | "spiral" | "recursive" => Some(LayoutKind::Dwindle),
            "auto" | "dynamic" | "smart" => Some(LayoutKind::Auto),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            LayoutKind::Scroll => "scroll",
            LayoutKind::MasterStack => "master-stack",
            LayoutKind::Dwindle => "dwindle",
            LayoutKind::Auto => "auto",
        }
    }

    /// The next selectable layout, used by the `layout next` action.
    pub fn next(self) -> LayoutKind {
        let index = Self::SELECTABLE
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0);
        Self::SELECTABLE[(index + 1) % Self::SELECTABLE.len()]
    }

    /// Resolve a dynamic layout for the current window count.
    ///
    /// One to three windows read best as a master plus a stack; from four
    /// windows on the spiral keeps every window usable. An explicit layout is
    /// returned unchanged.
    pub fn effective(self, tiled_windows: usize) -> LayoutKind {
        match self {
            LayoutKind::Auto if tiled_windows >= 4 => LayoutKind::Dwindle,
            LayoutKind::Auto => LayoutKind::MasterStack,
            other => other,
        }
    }
}

impl std::fmt::Display for LayoutKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for LayoutKind {
    type Err = crate::action::ActionParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        LayoutKind::parse(value).ok_or_else(|| {
            crate::action::ActionParseError(format!(
                "unknown layout '{value}' (expected scroll, master-stack, dwindle or auto)"
            ))
        })
    }
}

/// Gap used for a workspace with `tiled_windows` windows.
///
/// Smart gaps remove the gap entirely for a lone window (so it uses the whole
/// usable area) and tighten it once the layout gets dense. With smart gaps
/// disabled the configured gap is used verbatim.
pub fn smart_gap(config: &LayoutConfig, tiled_windows: usize) -> i32 {
    let gap = config.gap.max(0);
    if !config.smart_gaps {
        return gap;
    }
    match tiled_windows {
        0 | 1 => 0,
        2..=3 => gap,
        _ => (gap * 2 / 3).max(if gap > 0 { 1 } else { 0 }),
    }
}

/// Geometry of every window for a resolved layout.
///
/// [`LayoutKind::Scroll`] is owned by [`crate::layout::ScrollLayout`] (it also
/// tracks scroll offset), so it yields no placements here; [`LayoutKind::Auto`]
/// must be resolved with [`LayoutKind::effective`] first.
pub fn arrange(
    kind: LayoutKind,
    windows: &[WindowId],
    viewport: Rect,
    gap: i32,
    master_ratio: f64,
) -> Vec<Placement> {
    match kind {
        LayoutKind::MasterStack => master_stack(windows, viewport, gap, master_ratio),
        LayoutKind::Dwindle => dwindle(windows, viewport, gap),
        LayoutKind::Scroll | LayoutKind::Auto => Vec::new(),
    }
}

fn master_stack(
    windows: &[WindowId],
    viewport: Rect,
    gap: i32,
    master_ratio: f64,
) -> Vec<Placement> {
    let mut placements = Vec::with_capacity(windows.len());
    if windows.is_empty() || viewport.is_empty() {
        return placements;
    }
    if windows.len() == 1 {
        placements.push(Placement {
            window: windows[0],
            rect: viewport,
        });
        return placements;
    }

    let gap = gap.max(0).min(viewport.w.saturating_sub(2));
    let usable = (viewport.w - gap).max(2);
    let ratio = master_ratio.clamp(0.1, 0.9);
    let master_width = ((usable as f64 * ratio).round() as i32).clamp(1, usable - 1);
    let stack_width = usable - master_width;

    placements.push(Placement {
        window: windows[0],
        rect: Rect::new(viewport.x, viewport.y, master_width, viewport.h),
    });

    let stack_x = viewport.x + master_width + gap;
    let rows = windows.len() - 1;
    let available = (viewport.h - gap * (rows as i32 - 1)).max(rows as i32);
    let row_height = available / rows as i32;
    let mut y = viewport.y;
    for (index, window) in windows[1..].iter().enumerate() {
        let height = if index + 1 == rows {
            available - row_height * (rows as i32 - 1)
        } else {
            row_height
        };
        placements.push(Placement {
            window: *window,
            rect: Rect::new(stack_x, y, stack_width, height),
        });
        y += height + gap;
    }
    placements
}

fn dwindle(windows: &[WindowId], viewport: Rect, gap: i32) -> Vec<Placement> {
    let mut placements = Vec::with_capacity(windows.len());
    if windows.is_empty() || viewport.is_empty() {
        return placements;
    }
    dwindle_into(windows, viewport, gap.max(0), true, &mut placements);
    placements
}

fn dwindle_into(
    windows: &[WindowId],
    rect: Rect,
    gap: i32,
    horizontal: bool,
    placements: &mut Vec<Placement>,
) {
    if windows.len() == 1 {
        placements.push(Placement {
            window: windows[0],
            rect,
        });
        return;
    }
    let (first, rest) = split(rect, gap, horizontal);
    placements.push(Placement {
        window: windows[0],
        rect: first,
    });
    dwindle_into(&windows[1..], rest, gap, !horizontal, placements);
}

/// Split a rectangle in two along one axis, leaving a gap between the halves.
///
/// Both halves keep at least one pixel, so degenerate viewports cannot produce
/// empty windows or a stuck recursion.
fn split(rect: Rect, gap: i32, horizontal: bool) -> (Rect, Rect) {
    if horizontal {
        let gap = gap.min(rect.w.saturating_sub(2)).max(0);
        let usable = (rect.w - gap).max(2);
        let first_width = (usable / 2).max(1).min(usable - 1);
        let second_width = usable - first_width;
        (
            Rect::new(rect.x, rect.y, first_width, rect.h),
            Rect::new(rect.x + first_width + gap, rect.y, second_width, rect.h),
        )
    } else {
        let gap = gap.min(rect.h.saturating_sub(2)).max(0);
        let usable = (rect.h - gap).max(2);
        let first_height = (usable / 2).max(1).min(usable - 1);
        let second_height = usable - first_height;
        (
            Rect::new(rect.x, rect.y, rect.w, first_height),
            Rect::new(rect.x, rect.y + first_height + gap, rect.w, second_height),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: u64) -> WindowId {
        WindowId(id)
    }

    fn ids(count: usize) -> Vec<WindowId> {
        (1..=count as u64).map(window).collect()
    }

    #[test]
    fn every_layout_round_trips_through_text() {
        for kind in LayoutKind::SELECTABLE {
            assert_eq!(LayoutKind::parse(kind.as_str()), Some(kind));
            assert_eq!(kind.to_string(), kind.as_str());
        }
        assert_eq!(
            LayoutKind::parse("Master-Stack"),
            Some(LayoutKind::MasterStack)
        );
        assert_eq!(LayoutKind::parse("spiral"), Some(LayoutKind::Dwindle));
        assert_eq!(LayoutKind::parse("nope"), None);
    }

    #[test]
    fn layout_next_cycles_through_every_layout() {
        let mut kind = LayoutKind::Scroll;
        let mut seen = Vec::new();
        for _ in 0..LayoutKind::SELECTABLE.len() {
            seen.push(kind);
            kind = kind.next();
        }
        assert_eq!(seen, LayoutKind::SELECTABLE.to_vec());
        assert_eq!(kind, LayoutKind::Scroll);
    }

    #[test]
    fn auto_layout_resolves_by_window_count() {
        assert_eq!(LayoutKind::Auto.effective(0), LayoutKind::MasterStack);
        assert_eq!(LayoutKind::Auto.effective(3), LayoutKind::MasterStack);
        assert_eq!(LayoutKind::Auto.effective(4), LayoutKind::Dwindle);
        assert_eq!(LayoutKind::Auto.effective(12), LayoutKind::Dwindle);
        assert_eq!(LayoutKind::Scroll.effective(9), LayoutKind::Scroll);
        assert_eq!(
            LayoutKind::MasterStack.effective(9),
            LayoutKind::MasterStack
        );
    }

    #[test]
    fn smart_gaps_tighten_with_density_and_close_for_a_lone_window() {
        let mut config = LayoutConfig {
            gap: 12,
            smart_gaps: true,
            ..LayoutConfig::default()
        };

        assert_eq!(smart_gap(&config, 0), 0);
        assert_eq!(smart_gap(&config, 1), 0);
        assert_eq!(smart_gap(&config, 2), 12);
        assert_eq!(smart_gap(&config, 3), 12);
        assert_eq!(smart_gap(&config, 9), 8);

        config.smart_gaps = false;
        assert_eq!(smart_gap(&config, 1), 12);
        assert_eq!(smart_gap(&config, 9), 12);
    }

    #[test]
    fn master_stack_uses_the_whole_area_without_overlap() {
        let viewport = Rect::new(0, 0, 1200, 800);
        let placements = arrange(LayoutKind::MasterStack, &ids(3), viewport, 8, 0.6);
        assert_eq!(placements.len(), 3);

        let master = placements[0].rect;
        let first_stack = placements[1].rect;
        let second_stack = placements[2].rect;

        assert_eq!(master, Rect::new(0, 0, 715, 800));
        assert_eq!(first_stack.x, 723);
        assert_eq!(first_stack.w, 477);
        assert_eq!(first_stack.h, 396);
        assert_eq!(second_stack.y, 404);
        assert_eq!(second_stack.h, 396);
        assert_eq!(second_stack.bottom(), 800);
        assert_eq!(master.intersection(first_stack), None);
    }

    #[test]
    fn a_lone_window_fills_the_usable_area_in_every_layout() {
        let viewport = Rect::new(10, 20, 900, 700);
        for kind in [LayoutKind::MasterStack, LayoutKind::Dwindle] {
            let placements = arrange(kind, &ids(1), viewport, 24, 0.6);
            assert_eq!(placements.len(), 1);
            assert_eq!(placements[0].rect, viewport);
        }
    }

    #[test]
    fn empty_inputs_produce_no_geometry() {
        for kind in [
            LayoutKind::Scroll,
            LayoutKind::MasterStack,
            LayoutKind::Dwindle,
        ] {
            assert!(arrange(kind, &[], Rect::new(0, 0, 800, 600), 8, 0.6).is_empty());
        }
        assert!(arrange(LayoutKind::MasterStack, &ids(3), Rect::ZERO, 8, 0.6).is_empty());
    }

    #[test]
    fn dwindle_splits_recursively_and_alternates_axes() {
        let viewport = Rect::new(0, 0, 1000, 600);
        let placements = arrange(LayoutKind::Dwindle, &ids(4), viewport, 8, 0.6);
        assert_eq!(placements.len(), 4);

        assert_eq!(placements[0].rect, Rect::new(0, 0, 496, 600));
        assert_eq!(placements[1].rect, Rect::new(504, 0, 496, 296));
        assert_eq!(placements[2].rect, Rect::new(504, 304, 244, 296));
        assert_eq!(placements[3].rect, Rect::new(756, 304, 244, 296));

        for (index, placement) in placements.iter().enumerate() {
            assert!(placement.rect.w > 0 && placement.rect.h > 0);
            for other in &placements[index + 1..] {
                assert_eq!(
                    placement.rect.intersection(other.rect),
                    None,
                    "dwindle tiles never overlap"
                );
            }
        }
    }

    #[test]
    fn tiny_viewports_still_produce_valid_geometry() {
        for kind in [LayoutKind::MasterStack, LayoutKind::Dwindle] {
            for (width, height) in [(2, 2), (3, 40), (40, 3), (1, 1)] {
                let placements = arrange(kind, &ids(5), Rect::new(0, 0, width, height), 64, 0.6);
                assert_eq!(placements.len(), 5);
                for placement in placements {
                    assert!(placement.rect.w >= 0);
                    assert!(placement.rect.h >= 0);
                }
            }
        }
    }

    #[test]
    fn master_ratio_is_clamped_to_a_usable_range() {
        let viewport = Rect::new(0, 0, 1000, 600);
        let narrow = arrange(LayoutKind::MasterStack, &ids(2), viewport, 0, 0.0);
        let wide = arrange(LayoutKind::MasterStack, &ids(2), viewport, 0, 5.0);
        assert_eq!(narrow[0].rect.w, 100);
        assert_eq!(wide[0].rect.w, 900);
        assert!(narrow[1].rect.w > 0);
        assert!(wide[1].rect.w > 0);
    }
}
