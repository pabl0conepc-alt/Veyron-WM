use std::num::NonZeroU64;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WindowId(NonZeroU64);

impl WindowId {
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SplitAxis {
    /// Place the first child to the left of the second.
    Horizontal,
    /// Place the first child above the second.
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Placement {
    pub window: WindowId,
    pub rect: Rect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutError {
    DuplicateWindow(WindowId),
    MissingAnchor(WindowId),
    AnchorRequired,
    NoSplitForWindow(WindowId),
    InvalidSplitRatio(u16),
    InsufficientSpace { available: Size, required: Size },
    CoordinateOverflow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Node {
    Window(WindowId),
    Split {
        axis: SplitAxis,
        ratio_basis_points: u16,
        first: Box<Node>,
        second: Box<Node>,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LayoutTree {
    root: Option<Node>,
}

impl LayoutTree {
    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    pub fn len(&self) -> usize {
        self.root.as_ref().map_or(0, Node::len)
    }

    /// Add a window beside `anchor`. The anchor stays in the first half of the split.
    /// An empty tree accepts exactly one window with no anchor.
    pub fn insert_after(
        &mut self,
        anchor: Option<WindowId>,
        window: WindowId,
        axis: SplitAxis,
    ) -> Result<(), LayoutError> {
        if self.contains(window) {
            return Err(LayoutError::DuplicateWindow(window));
        }

        match (&mut self.root, anchor) {
            (None, None) => {
                self.root = Some(Node::Window(window));
                Ok(())
            }
            (None, Some(anchor)) => Err(LayoutError::MissingAnchor(anchor)),
            (Some(_), None) => Err(LayoutError::AnchorRequired),
            (Some(root), Some(anchor)) => {
                if root.insert_after(anchor, window, axis) {
                    Ok(())
                } else {
                    Err(LayoutError::MissingAnchor(anchor))
                }
            }
        }
    }

    /// Remove a window. When a split loses one child, its other child takes its place.
    pub fn remove(&mut self, window: WindowId) -> Result<(), LayoutError> {
        if !self.contains(window) {
            return Err(LayoutError::MissingAnchor(window));
        }
        let Some(root) = self.root.take() else {
            return Err(LayoutError::MissingAnchor(window));
        };
        self.root = root.remove(window).ok();
        Ok(())
    }

    pub fn contains(&self, window: WindowId) -> bool {
        self.root.as_ref().is_some_and(|root| root.contains(window))
    }

    /// Resize the closest split containing `window`; the value is the first child's share
    /// in basis points, clamped to keep at least 10% for each child.
    pub fn set_split_ratio(
        &mut self,
        window: WindowId,
        ratio_basis_points: u16,
    ) -> Result<(), LayoutError> {
        if !(1_000..=9_000).contains(&ratio_basis_points) {
            return Err(LayoutError::InvalidSplitRatio(ratio_basis_points));
        }
        let Some(root) = &mut self.root else {
            return Err(LayoutError::MissingAnchor(window));
        };
        if !root.contains(window) {
            return Err(LayoutError::MissingAnchor(window));
        }
        if root.set_split_ratio(window, ratio_basis_points) {
            Ok(())
        } else {
            Err(LayoutError::NoSplitForWindow(window))
        }
    }

    /// Calculate a deterministic split layout while enforcing per-window minimum size.
    pub fn arrange(&self, bounds: Rect, minimum: Size) -> Result<Vec<Placement>, LayoutError> {
        let Some(root) = &self.root else {
            return Ok(Vec::new());
        };

        let required = root.minimum_size(minimum);
        let available = Size {
            width: bounds.width,
            height: bounds.height,
        };
        if available.width < required.width || available.height < required.height {
            return Err(LayoutError::InsufficientSpace {
                available,
                required,
            });
        }

        let mut placements = Vec::with_capacity(root.len());
        root.arrange(bounds, minimum, &mut placements)?;
        Ok(placements)
    }
}

impl Node {
    fn len(&self) -> usize {
        match self {
            Self::Window(_) => 1,
            Self::Split { first, second, .. } => first.len() + second.len(),
        }
    }

    fn contains(&self, window: WindowId) -> bool {
        match self {
            Self::Window(id) => *id == window,
            Self::Split { first, second, .. } => first.contains(window) || second.contains(window),
        }
    }

    fn set_split_ratio(&mut self, window: WindowId, ratio_basis_points: u16) -> bool {
        let Self::Split {
            ratio_basis_points: ratio,
            first,
            second,
            ..
        } = self
        else {
            return false;
        };

        if first.contains(window) && first.set_split_ratio(window, ratio_basis_points) {
            return true;
        }
        if second.contains(window) && second.set_split_ratio(window, ratio_basis_points) {
            return true;
        }
        if first.contains(window) || second.contains(window) {
            *ratio = ratio_basis_points;
            true
        } else {
            false
        }
    }

    fn insert_after(&mut self, anchor: WindowId, window: WindowId, axis: SplitAxis) -> bool {
        match self {
            Self::Window(id) if *id == anchor => {
                let first = Box::new(Self::Window(*id));
                let second = Box::new(Self::Window(window));
                *self = Self::Split {
                    axis,
                    ratio_basis_points: 5_000,
                    first,
                    second,
                };
                true
            }
            Self::Window(_) => false,
            Self::Split { first, second, .. } => {
                if first.contains(anchor) {
                    first.insert_after(anchor, window, axis)
                } else {
                    second.insert_after(anchor, window, axis)
                }
            }
        }
    }

    fn remove(self, window: WindowId) -> Result<Self, ()> {
        match self {
            Self::Window(id) if id == window => Err(()),
            leaf @ Self::Window(_) => Ok(leaf),
            Self::Split {
                axis,
                ratio_basis_points,
                first,
                second,
            } => {
                let first = match first.remove(window) {
                    Ok(first) => first,
                    Err(()) => return Ok(*second),
                };
                let second = match second.remove(window) {
                    Ok(second) => second,
                    Err(()) => return Ok(first),
                };
                Ok(Self::Split {
                    axis,
                    ratio_basis_points,
                    first: Box::new(first),
                    second: Box::new(second),
                })
            }
        }
    }

    fn minimum_size(&self, minimum: Size) -> Size {
        match self {
            Self::Window(_) => minimum,
            Self::Split {
                axis,
                first,
                second,
                ..
            } => {
                let first = first.minimum_size(minimum);
                let second = second.minimum_size(minimum);
                match axis {
                    SplitAxis::Horizontal => Size {
                        width: first.width.saturating_add(second.width),
                        height: first.height.max(second.height),
                    },
                    SplitAxis::Vertical => Size {
                        width: first.width.max(second.width),
                        height: first.height.saturating_add(second.height),
                    },
                }
            }
        }
    }

    fn arrange(
        &self,
        bounds: Rect,
        minimum: Size,
        placements: &mut Vec<Placement>,
    ) -> Result<(), LayoutError> {
        match self {
            Self::Window(window) => placements.push(Placement {
                window: *window,
                rect: bounds,
            }),
            Self::Split {
                axis,
                ratio_basis_points,
                first,
                second,
            } => {
                let first_minimum = first.minimum_size(minimum);
                let second_minimum = second.minimum_size(minimum);
                match axis {
                    SplitAxis::Horizontal => {
                        let required = Size {
                            width: first_minimum.width.saturating_add(second_minimum.width),
                            height: first_minimum.height.max(second_minimum.height),
                        };
                        let first_width = split_extent(
                            bounds.width,
                            first_minimum.width,
                            second_minimum.width,
                            *ratio_basis_points,
                            Size {
                                width: bounds.width,
                                height: bounds.height,
                            },
                            required,
                        )?;
                        let second_x = bounds
                            .x
                            .checked_add(
                                i32::try_from(first_width)
                                    .map_err(|_| LayoutError::CoordinateOverflow)?,
                            )
                            .ok_or(LayoutError::CoordinateOverflow)?;
                        first.arrange(
                            Rect {
                                width: first_width,
                                ..bounds
                            },
                            minimum,
                            placements,
                        )?;
                        second.arrange(
                            Rect {
                                x: second_x,
                                width: bounds.width - first_width,
                                ..bounds
                            },
                            minimum,
                            placements,
                        )?;
                    }
                    SplitAxis::Vertical => {
                        let required = Size {
                            width: first_minimum.width.max(second_minimum.width),
                            height: first_minimum.height.saturating_add(second_minimum.height),
                        };
                        let first_height = split_extent(
                            bounds.height,
                            first_minimum.height,
                            second_minimum.height,
                            *ratio_basis_points,
                            Size {
                                width: bounds.width,
                                height: bounds.height,
                            },
                            required,
                        )?;
                        let second_y = bounds
                            .y
                            .checked_add(
                                i32::try_from(first_height)
                                    .map_err(|_| LayoutError::CoordinateOverflow)?,
                            )
                            .ok_or(LayoutError::CoordinateOverflow)?;
                        first.arrange(
                            Rect {
                                height: first_height,
                                ..bounds
                            },
                            minimum,
                            placements,
                        )?;
                        second.arrange(
                            Rect {
                                y: second_y,
                                height: bounds.height - first_height,
                                ..bounds
                            },
                            minimum,
                            placements,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn split_extent(
    total: u32,
    first_minimum: u32,
    second_minimum: u32,
    ratio_basis_points: u16,
    available: Size,
    required: Size,
) -> Result<u32, LayoutError> {
    if first_minimum > total || second_minimum > total - first_minimum {
        return Err(LayoutError::InsufficientSpace {
            available,
            required,
        });
    }
    let largest_first = total - second_minimum;
    let ratio_extent = ((u64::from(total) * u64::from(ratio_basis_points)) / 10_000) as u32;
    Ok(ratio_extent.max(first_minimum).min(largest_first))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: u64) -> WindowId {
        WindowId::new(NonZeroU64::new(value).expect("test ids are nonzero"))
    }

    fn rect(x: i32, y: i32, width: u32, height: u32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    const MINIMUM: Size = Size {
        width: 10,
        height: 10,
    };

    #[test]
    fn inserts_first_window_and_fills_output() {
        let mut tree = LayoutTree::default();
        tree.insert_after(None, id(1), SplitAxis::Horizontal)
            .unwrap();

        assert_eq!(tree.len(), 1);
        assert_eq!(
            tree.arrange(rect(0, 0, 800, 600), MINIMUM).unwrap(),
            vec![Placement {
                window: id(1),
                rect: rect(0, 0, 800, 600),
            }]
        );
    }

    #[test]
    fn horizontal_split_is_deterministic_and_covers_bounds() {
        let mut tree = LayoutTree::default();
        tree.insert_after(None, id(1), SplitAxis::Horizontal)
            .unwrap();
        tree.insert_after(Some(id(1)), id(2), SplitAxis::Horizontal)
            .unwrap();

        assert_eq!(
            tree.arrange(rect(5, 7, 801, 600), MINIMUM).unwrap(),
            vec![
                Placement {
                    window: id(1),
                    rect: rect(5, 7, 400, 600),
                },
                Placement {
                    window: id(2),
                    rect: rect(405, 7, 401, 600),
                },
            ]
        );
    }

    #[test]
    fn nested_split_respects_each_windows_minimum_size() {
        let mut tree = LayoutTree::default();
        tree.insert_after(None, id(1), SplitAxis::Horizontal)
            .unwrap();
        tree.insert_after(Some(id(1)), id(2), SplitAxis::Horizontal)
            .unwrap();
        tree.insert_after(Some(id(2)), id(3), SplitAxis::Vertical)
            .unwrap();

        let result = tree.arrange(rect(0, 0, 120, 80), MINIMUM).unwrap();
        assert_eq!(result[0].rect, rect(0, 0, 60, 80));
        assert_eq!(result[1].rect, rect(60, 0, 60, 40));
        assert_eq!(result[2].rect, rect(60, 40, 60, 40));
    }

    #[test]
    fn resizing_split_changes_first_child_share() {
        let mut tree = LayoutTree::default();
        tree.insert_after(None, id(1), SplitAxis::Horizontal)
            .unwrap();
        tree.insert_after(Some(id(1)), id(2), SplitAxis::Horizontal)
            .unwrap();
        tree.set_split_ratio(id(1), 3_000).unwrap();

        let result = tree.arrange(rect(0, 0, 1_000, 600), MINIMUM).unwrap();
        assert_eq!(result[0].rect, rect(0, 0, 300, 600));
        assert_eq!(result[1].rect, rect(300, 0, 700, 600));
    }

    #[test]
    fn resizing_nested_window_targets_its_closest_split() {
        let mut tree = LayoutTree::default();
        tree.insert_after(None, id(1), SplitAxis::Horizontal)
            .unwrap();
        tree.insert_after(Some(id(1)), id(2), SplitAxis::Horizontal)
            .unwrap();
        tree.insert_after(Some(id(2)), id(3), SplitAxis::Vertical)
            .unwrap();
        tree.set_split_ratio(id(2), 2_500).unwrap();

        let result = tree.arrange(rect(0, 0, 120, 80), MINIMUM).unwrap();
        assert_eq!(result[0].rect, rect(0, 0, 60, 80));
        assert_eq!(result[1].rect, rect(60, 0, 60, 20));
        assert_eq!(result[2].rect, rect(60, 20, 60, 60));
    }

    #[test]
    fn split_resize_is_limited_by_minimum_window_size() {
        let mut tree = LayoutTree::default();
        tree.insert_after(None, id(1), SplitAxis::Horizontal)
            .unwrap();
        tree.insert_after(Some(id(1)), id(2), SplitAxis::Horizontal)
            .unwrap();
        tree.set_split_ratio(id(1), 1_000).unwrap();

        let result = tree.arrange(rect(0, 0, 100, 60), MINIMUM).unwrap();
        assert_eq!(result[0].rect.width, MINIMUM.width);
        assert_eq!(result[1].rect.width, 90);
    }

    #[test]
    fn invalid_split_ratio_does_not_change_layout() {
        let mut tree = LayoutTree::default();
        tree.insert_after(None, id(1), SplitAxis::Horizontal)
            .unwrap();
        tree.insert_after(Some(id(1)), id(2), SplitAxis::Horizontal)
            .unwrap();
        let original = tree.clone();

        assert_eq!(
            tree.set_split_ratio(id(1), 9_001),
            Err(LayoutError::InvalidSplitRatio(9_001))
        );
        assert_eq!(tree, original);
    }

    #[test]
    fn removal_promotes_sibling_and_rejects_unknown_window() {
        let mut tree = LayoutTree::default();
        tree.insert_after(None, id(1), SplitAxis::Horizontal)
            .unwrap();
        tree.insert_after(Some(id(1)), id(2), SplitAxis::Horizontal)
            .unwrap();
        tree.remove(id(1)).unwrap();

        assert_eq!(tree.len(), 1);
        assert!(!tree.contains(id(1)));
        assert_eq!(
            tree.arrange(rect(0, 0, 80, 60), MINIMUM).unwrap()[0],
            Placement {
                window: id(2),
                rect: rect(0, 0, 80, 60),
            }
        );
        assert_eq!(tree.remove(id(1)), Err(LayoutError::MissingAnchor(id(1))));
    }

    #[test]
    fn rejects_duplicate_and_missing_anchor_without_mutating_tree() {
        let mut tree = LayoutTree::default();
        tree.insert_after(None, id(1), SplitAxis::Horizontal)
            .unwrap();

        assert_eq!(
            tree.insert_after(Some(id(1)), id(1), SplitAxis::Vertical),
            Err(LayoutError::DuplicateWindow(id(1)))
        );
        assert_eq!(
            tree.insert_after(Some(id(9)), id(2), SplitAxis::Vertical),
            Err(LayoutError::MissingAnchor(id(9)))
        );
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn reports_when_output_cannot_fit_minimum_sizes() {
        let mut tree = LayoutTree::default();
        tree.insert_after(None, id(1), SplitAxis::Horizontal)
            .unwrap();
        tree.insert_after(Some(id(1)), id(2), SplitAxis::Horizontal)
            .unwrap();

        assert_eq!(
            tree.arrange(rect(0, 0, 19, 60), MINIMUM),
            Err(LayoutError::InsufficientSpace {
                available: Size {
                    width: 19,
                    height: 60,
                },
                required: Size {
                    width: 20,
                    height: 10,
                },
            })
        );
    }
}
