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
//! - [`LayoutKind::Grid`] arranges windows in a near-square grid.
//! - [`LayoutKind::CenterTile`] centres the master between two stack columns.
//! - [`LayoutKind::Deck`] is master-stack showing one stack window at a time.
//! - [`LayoutKind::Monocle`] shows only the focused window.
//! - [`LayoutKind::Floating`] tiles nothing; the window manager draws every
//!   window of the workspace at its floating geometry.
//! - [`LayoutKind::Auto`] resolves per window count (dynamic tiling).
//!
//! Monocle and deck place only some of the windows they are given. A window
//! without a placement is not drawn, which is how the compositor hides it.
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
    /// A near-square grid; the last row is stretched to fill the width.
    Grid,
    /// The master in a centred column, the stack alternating between a right
    /// and a left column (mango `center_tile`, dwm `centeredmaster`).
    CenterTile,
    /// A master area plus a single visible stack window (dwm `deck`).
    Deck,
    /// The focused window fills the area and every other one is hidden.
    Monocle,
    /// No tiling: every window of the workspace floats (COSMIC style).
    Floating,
    /// Pick [`LayoutKind::MasterStack`] or [`LayoutKind::Dwindle`] from the
    /// number of tiled windows (dynamic tiling).
    Auto,
}

impl LayoutKind {
    /// Every layout a user can select explicitly, in `layout next` order.
    ///
    /// The original four keep their order so existing muscle memory still
    /// works; the newer layouts follow `auto`.
    pub const SELECTABLE: [LayoutKind; 9] = [
        LayoutKind::Scroll,
        LayoutKind::MasterStack,
        LayoutKind::Dwindle,
        LayoutKind::Auto,
        LayoutKind::Grid,
        LayoutKind::CenterTile,
        LayoutKind::Deck,
        LayoutKind::Monocle,
        LayoutKind::Floating,
    ];

    /// Parse the canonical name, accepting the aliases people are used to.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "scroll" | "scrolling" | "strip" => Some(LayoutKind::Scroll),
            "master" | "master-stack" | "masterstack" | "ms" => Some(LayoutKind::MasterStack),
            "dwindle" | "spiral" | "recursive" => Some(LayoutKind::Dwindle),
            "grid" | "gapless-grid" => Some(LayoutKind::Grid),
            "center-tile" | "centertile" | "center_tile" | "centered-master" => {
                Some(LayoutKind::CenterTile)
            }
            "deck" => Some(LayoutKind::Deck),
            "monocle" | "max" => Some(LayoutKind::Monocle),
            "floating" | "float" | "free" => Some(LayoutKind::Floating),
            "auto" | "dynamic" | "smart" => Some(LayoutKind::Auto),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            LayoutKind::Scroll => "scroll",
            LayoutKind::MasterStack => "master-stack",
            LayoutKind::Dwindle => "dwindle",
            LayoutKind::Grid => "grid",
            LayoutKind::CenterTile => "center-tile",
            LayoutKind::Deck => "deck",
            LayoutKind::Monocle => "monocle",
            LayoutKind::Floating => "floating",
            LayoutKind::Auto => "auto",
        }
    }

    /// Whether the layout leaves windows untiled, at their floating geometry.
    pub fn is_floating(self) -> bool {
        self == LayoutKind::Floating
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
                "unknown layout '{value}' (expected scroll, master-stack, dwindle, auto, \
                 grid, center-tile, deck, monocle or floating)"
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

/// Geometry of the windows a resolved layout shows, in window order.
///
/// `shown` is the window monocle and deck keep visible (normally the focused
/// one); the other layouts ignore it. Windows those two layouts hide get no
/// placement at all.
///
/// [`LayoutKind::Scroll`] is owned by [`crate::layout::ScrollLayout`] (it also
/// tracks scroll offset) and [`LayoutKind::Floating`] by the window manager's
/// floating geometry, so both yield no placements here; [`LayoutKind::Auto`]
/// must be resolved with [`LayoutKind::effective`] first.
pub fn arrange(
    kind: LayoutKind,
    windows: &[WindowId],
    shown: Option<WindowId>,
    viewport: Rect,
    gap: i32,
    master_ratio: f64,
) -> Vec<Placement> {
    match kind {
        LayoutKind::MasterStack => master_stack(windows, viewport, gap, master_ratio),
        LayoutKind::Dwindle => dwindle(windows, viewport, gap),
        LayoutKind::Grid => grid(windows, viewport, gap),
        LayoutKind::CenterTile => center_tile(windows, viewport, gap, master_ratio),
        LayoutKind::Deck => deck(windows, shown, viewport, gap, master_ratio),
        LayoutKind::Monocle => monocle(windows, shown, viewport),
        LayoutKind::Scroll | LayoutKind::Floating | LayoutKind::Auto => Vec::new(),
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

/// Rows of up to `ceil(sqrt(n))` windows; the last row may hold fewer windows,
/// which share its whole width.
fn grid(windows: &[WindowId], viewport: Rect, gap: i32) -> Vec<Placement> {
    let mut placements = Vec::with_capacity(windows.len());
    if windows.is_empty() || viewport.is_empty() {
        return placements;
    }
    let mut columns = 1;
    while columns * columns < windows.len() {
        columns += 1;
    }
    let rows = windows.len().div_ceil(columns);
    for (row, (y, height)) in spans(viewport.y, viewport.h, rows, gap)
        .into_iter()
        .enumerate()
    {
        let start = row * columns;
        let cells = &windows[start..(start + columns).min(windows.len())];
        let widths = spans(viewport.x, viewport.w, cells.len(), gap);
        for (window, (x, width)) in cells.iter().zip(widths) {
            placements.push(Placement {
                window: *window,
                rect: Rect::new(x, y, width, height),
            });
        }
    }
    placements
}

/// The master in the middle at `master_ratio` of the width, the stack split
/// between a right and a left column (first stack window on the right).
///
/// The master keeps the same rectangle whatever the window count, so it never
/// jumps while windows open and close; a lone window is simply centred.
fn center_tile(
    windows: &[WindowId],
    viewport: Rect,
    gap: i32,
    master_ratio: f64,
) -> Vec<Placement> {
    let mut placements = Vec::with_capacity(windows.len());
    if windows.is_empty() || viewport.is_empty() {
        return placements;
    }

    let gap = gap.max(0).min((viewport.w - 3).max(0) / 2);
    let usable = (viewport.w - 2 * gap).max(3);
    let ratio = master_ratio.clamp(0.1, 0.9);
    let master_width = ((usable as f64 * ratio).round() as i32).clamp(1, usable - 2);
    let side = usable - master_width;
    let left_width = side / 2;
    let right_width = side - left_width;
    let master_x = viewport.x + left_width + gap;
    let right_x = master_x + master_width + gap;

    let stack = &windows[1..];
    let mut right = spans(viewport.y, viewport.h, stack.len().div_ceil(2), gap).into_iter();
    let mut left = spans(viewport.y, viewport.h, stack.len() / 2, gap).into_iter();

    placements.push(Placement {
        window: windows[0],
        rect: Rect::new(master_x, viewport.y, master_width, viewport.h),
    });
    for (index, window) in stack.iter().enumerate() {
        let (x, width, span) = if index % 2 == 0 {
            (right_x, right_width, right.next())
        } else {
            (viewport.x, left_width, left.next())
        };
        if let Some((y, height)) = span {
            placements.push(Placement {
                window: *window,
                rect: Rect::new(x, y, width, height),
            });
        }
    }
    placements
}

/// Master-stack with a single visible stack window: `shown` when it belongs
/// to the stack, otherwise the first stack window. The rest stay hidden.
fn deck(
    windows: &[WindowId],
    shown: Option<WindowId>,
    viewport: Rect,
    gap: i32,
    master_ratio: f64,
) -> Vec<Placement> {
    if windows.len() <= 2 {
        return master_stack(windows, viewport, gap, master_ratio);
    }
    let stack = &windows[1..];
    let visible = shown
        .filter(|window| stack.contains(window))
        .unwrap_or(stack[0]);
    master_stack(&[windows[0], visible], viewport, gap, master_ratio)
}

/// Only `shown` (or the first window when it is not part of the layout) is
/// placed, covering the whole area.
fn monocle(windows: &[WindowId], shown: Option<WindowId>, viewport: Rect) -> Vec<Placement> {
    if windows.is_empty() || viewport.is_empty() {
        return Vec::new();
    }
    let window = shown
        .filter(|window| windows.contains(window))
        .unwrap_or(windows[0]);
    vec![Placement {
        window,
        rect: viewport,
    }]
}

/// Cut `length` pixels starting at `start` into `count` runs separated by
/// `gap`. Every run keeps at least one pixel (the gap shrinks first) and the
/// last run absorbs the rounding, so the runs end exactly at `start + length`
/// whenever there is room for them.
fn spans(start: i32, length: i32, count: usize, gap: i32) -> Vec<(i32, i32)> {
    if count == 0 {
        return Vec::new();
    }
    let count = count as i32;
    let gap = if count > 1 {
        gap.max(0).min((length - count).max(0) / (count - 1))
    } else {
        0
    };
    let available = (length - gap * (count - 1)).max(count);
    let each = available / count;
    let mut position = start;
    (0..count)
        .map(|index| {
            let size = if index + 1 == count {
                available - each * (count - 1)
            } else {
                each
            };
            let span = (position, size);
            position += size + gap;
            span
        })
        .collect()
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
        let placements = arrange(LayoutKind::MasterStack, &ids(3), None, viewport, 8, 0.6);
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
        for kind in [
            LayoutKind::MasterStack,
            LayoutKind::Dwindle,
            LayoutKind::Grid,
            LayoutKind::Deck,
            LayoutKind::Monocle,
        ] {
            let placements = arrange(kind, &ids(1), None, viewport, 24, 0.6);
            assert_eq!(placements.len(), 1, "{kind}");
            assert_eq!(placements[0].rect, viewport, "{kind}");
        }
    }

    #[test]
    fn empty_inputs_produce_no_geometry() {
        for kind in LayoutKind::SELECTABLE {
            assert!(arrange(kind, &[], None, Rect::new(0, 0, 800, 600), 8, 0.6).is_empty());
            assert!(arrange(kind, &ids(3), None, Rect::ZERO, 8, 0.6).is_empty());
        }
    }

    #[test]
    fn scroll_floating_and_unresolved_auto_place_nothing_here() {
        let viewport = Rect::new(0, 0, 800, 600);
        for kind in [LayoutKind::Scroll, LayoutKind::Floating, LayoutKind::Auto] {
            assert!(arrange(kind, &ids(3), None, viewport, 8, 0.6).is_empty());
        }
        assert!(LayoutKind::Floating.is_floating());
        assert!(!LayoutKind::Monocle.is_floating());
    }

    #[test]
    fn new_layouts_parse_with_their_aliases_and_bad_names_are_rejected() {
        assert_eq!(LayoutKind::parse("grid"), Some(LayoutKind::Grid));
        assert_eq!(
            LayoutKind::parse("center_tile"),
            Some(LayoutKind::CenterTile)
        );
        assert_eq!(
            LayoutKind::parse("centered-master"),
            Some(LayoutKind::CenterTile)
        );
        assert_eq!(LayoutKind::parse("Deck"), Some(LayoutKind::Deck));
        assert_eq!(LayoutKind::parse("max"), Some(LayoutKind::Monocle));
        assert_eq!(LayoutKind::parse("float"), Some(LayoutKind::Floating));
        assert_eq!(LayoutKind::parse("tabbed"), None);

        let error = "tabbed".parse::<LayoutKind>().expect_err("unknown layout");
        assert!(error.0.contains("center-tile"), "{}", error.0);
    }

    #[test]
    fn grid_is_near_square_and_stretches_the_last_row() {
        let viewport = Rect::new(0, 0, 1000, 600);

        let four = arrange(LayoutKind::Grid, &ids(4), None, viewport, 8, 0.6);
        let rects: Vec<Rect> = four.iter().map(|placement| placement.rect).collect();
        assert_eq!(
            rects,
            vec![
                Rect::new(0, 0, 496, 296),
                Rect::new(504, 0, 496, 296),
                Rect::new(0, 304, 496, 296),
                Rect::new(504, 304, 496, 296),
            ]
        );

        // Three windows: two columns, the lone window of the second row takes
        // the whole width.
        let three = arrange(LayoutKind::Grid, &ids(3), None, viewport, 8, 0.6);
        assert_eq!(three[2].rect, Rect::new(0, 304, 1000, 296));

        // Five windows: three columns, then two stretched cells.
        let five = arrange(LayoutKind::Grid, &ids(5), None, viewport, 8, 0.6);
        let rects: Vec<Rect> = five.iter().map(|placement| placement.rect).collect();
        assert_eq!(
            rects,
            vec![
                Rect::new(0, 0, 328, 296),
                Rect::new(336, 0, 328, 296),
                Rect::new(672, 0, 328, 296),
                Rect::new(0, 304, 496, 296),
                Rect::new(504, 304, 496, 296),
            ]
        );

        // Seven windows: a 3x3 grid whose last row holds one full-width cell;
        // the last row absorbs the rounding so the grid ends at the bottom.
        let seven = arrange(LayoutKind::Grid, &ids(7), None, viewport, 8, 0.6);
        assert_eq!(seven[6].rect, Rect::new(0, 404, 1000, 196));
        assert_eq!(seven[0].rect.h, 194);

        let two = arrange(LayoutKind::Grid, &ids(2), None, viewport, 8, 0.6);
        assert_eq!(two[0].rect, Rect::new(0, 0, 496, 600));
        assert_eq!(two[1].rect, Rect::new(504, 0, 496, 600));
    }

    #[test]
    fn grid_never_overlaps_and_stays_inside_the_viewport() {
        let viewport = Rect::new(20, 30, 1280, 720);
        for count in 1..=12 {
            let placements = arrange(LayoutKind::Grid, &ids(count), None, viewport, 6, 0.6);
            assert_eq!(placements.len(), count);
            for (index, placement) in placements.iter().enumerate() {
                assert_eq!(
                    placement.rect.intersection(viewport),
                    Some(placement.rect),
                    "{count} windows: cell {index} inside the viewport"
                );
                for other in &placements[index + 1..] {
                    assert_eq!(placement.rect.intersection(other.rect), None);
                }
            }
            let bottom = placements.iter().map(|p| p.rect.bottom()).max();
            assert_eq!(bottom, Some(viewport.bottom()), "{count} windows");
        }
    }

    #[test]
    fn center_tile_keeps_the_master_centred_and_alternates_the_stack() {
        let viewport = Rect::new(0, 0, 1200, 800);
        let master = Rect::new(305, 0, 590, 800);

        let one = arrange(LayoutKind::CenterTile, &ids(1), None, viewport, 10, 0.5);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].rect, master, "a lone window is centred");

        let two = arrange(LayoutKind::CenterTile, &ids(2), None, viewport, 10, 0.5);
        assert_eq!(two[0].rect, master, "the master does not move");
        assert_eq!(two[1].rect, Rect::new(905, 0, 295, 800));

        let three = arrange(LayoutKind::CenterTile, &ids(3), None, viewport, 10, 0.5);
        assert_eq!(three[0].rect, master);
        assert_eq!(three[1].rect, Rect::new(905, 0, 295, 800), "right first");
        assert_eq!(three[2].rect, Rect::new(0, 0, 295, 800), "then left");

        let five = arrange(LayoutKind::CenterTile, &ids(5), None, viewport, 10, 0.5);
        let rects: Vec<Rect> = five.iter().map(|placement| placement.rect).collect();
        assert_eq!(
            rects,
            vec![
                master,
                Rect::new(905, 0, 295, 395),
                Rect::new(0, 0, 295, 395),
                Rect::new(905, 405, 295, 395),
                Rect::new(0, 405, 295, 395),
            ]
        );

        // An odd stack leaves the right column one window taller.
        let four = arrange(LayoutKind::CenterTile, &ids(4), None, viewport, 10, 0.5);
        assert_eq!(four[2].rect, Rect::new(0, 0, 295, 800));
        assert_eq!(four[3].rect, Rect::new(905, 405, 295, 395));
        for (index, placement) in four.iter().enumerate() {
            for other in &four[index + 1..] {
                assert_eq!(placement.rect.intersection(other.rect), None);
            }
        }
    }

    #[test]
    fn deck_shows_the_master_and_one_stack_window() {
        let viewport = Rect::new(0, 0, 1200, 800);
        let windows = ids(4);

        let first = arrange(LayoutKind::Deck, &windows, None, viewport, 8, 0.6);
        assert_eq!(first.len(), 2, "the other stack windows are hidden");
        assert_eq!(first[0].window, window(1));
        assert_eq!(first[0].rect, Rect::new(0, 0, 715, 800));
        assert_eq!(first[1].window, window(2));
        assert_eq!(first[1].rect, Rect::new(723, 0, 477, 800));

        let focused = arrange(
            LayoutKind::Deck,
            &windows,
            Some(window(4)),
            viewport,
            8,
            0.6,
        );
        assert_eq!(focused[1].window, window(4));
        assert_eq!(focused[1].rect, Rect::new(723, 0, 477, 800));

        // Focusing the master keeps showing the first stack window.
        let master = arrange(
            LayoutKind::Deck,
            &windows,
            Some(window(1)),
            viewport,
            8,
            0.6,
        );
        assert_eq!(master[1].window, window(2));

        // Two windows are exactly master-stack.
        assert_eq!(
            arrange(LayoutKind::Deck, &ids(2), None, viewport, 8, 0.6),
            arrange(LayoutKind::MasterStack, &ids(2), None, viewport, 8, 0.6)
        );
    }

    #[test]
    fn monocle_places_only_the_shown_window() {
        let viewport = Rect::new(12, 12, 1000, 700);
        let windows = ids(3);

        let shown = arrange(
            LayoutKind::Monocle,
            &windows,
            Some(window(2)),
            viewport,
            8,
            0.6,
        );
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].window, window(2));
        assert_eq!(shown[0].rect, viewport);

        let fallback = arrange(LayoutKind::Monocle, &windows, None, viewport, 8, 0.6);
        assert_eq!(fallback[0].window, window(1));
        let unknown = arrange(
            LayoutKind::Monocle,
            &windows,
            Some(window(9)),
            viewport,
            8,
            0.6,
        );
        assert_eq!(unknown[0].window, window(1));
    }

    #[test]
    fn spans_cover_the_length_and_shrink_the_gap_when_cramped() {
        assert_eq!(spans(0, 100, 0, 8), Vec::new());
        assert_eq!(spans(5, 100, 1, 8), vec![(5, 100)]);
        assert_eq!(spans(0, 100, 3, 8), vec![(0, 28), (36, 28), (72, 28)]);
        // Ten pixels cannot hold three cells and two 8px gaps: the gap shrinks.
        let cramped = spans(0, 10, 3, 8);
        assert_eq!(cramped, vec![(0, 1), (4, 1), (8, 2)]);
        assert!(cramped.iter().all(|(_, size)| *size >= 1));
    }

    #[test]
    fn dwindle_splits_recursively_and_alternates_axes() {
        let viewport = Rect::new(0, 0, 1000, 600);
        let placements = arrange(LayoutKind::Dwindle, &ids(4), None, viewport, 8, 0.6);
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
        for (kind, visible) in [
            (LayoutKind::MasterStack, 5),
            (LayoutKind::Dwindle, 5),
            (LayoutKind::Grid, 5),
            (LayoutKind::CenterTile, 5),
            (LayoutKind::Deck, 2),
            (LayoutKind::Monocle, 1),
        ] {
            for (width, height) in [(2, 2), (3, 40), (40, 3), (1, 1)] {
                let viewport = Rect::new(0, 0, width, height);
                let placements = arrange(kind, &ids(5), None, viewport, 64, 0.6);
                assert_eq!(placements.len(), visible, "{kind}");
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
        let narrow = arrange(LayoutKind::MasterStack, &ids(2), None, viewport, 0, 0.0);
        let wide = arrange(LayoutKind::MasterStack, &ids(2), None, viewport, 0, 5.0);
        assert_eq!(narrow[0].rect.w, 100);
        assert_eq!(wide[0].rect.w, 900);
        assert!(narrow[1].rect.w > 0);
        assert!(wide[1].rect.w > 0);
    }
}
