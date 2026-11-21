use std::collections::BTreeMap;

use super::layout::{LayoutError, LayoutTree, Placement, Rect, Size, SplitAxis, WindowId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowMode {
    Tiled,
    Floating,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManagedPlacement {
    pub window: WindowId,
    pub rect: Rect,
    pub mode: WindowMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowManagerError {
    DuplicateWindow(WindowId),
    UnknownWindow(WindowId),
    FloatingGeometryRequired,
    NotFloating(WindowId),
    NotTiled(WindowId),
    Layout(LayoutError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowSnapshot {
    id: WindowId,
    mode: WindowMode,
    floating_rect: Option<Rect>,
}

impl From<LayoutError> for WindowManagerError {
    fn from(error: LayoutError) -> Self {
        Self::Layout(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WindowState {
    mode: WindowMode,
    floating_rect: Option<Rect>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WindowManager {
    layout: LayoutTree,
    windows: BTreeMap<WindowId, WindowState>,
    focus_history: Vec<WindowId>,
    floating_order: Vec<WindowId>,
}

impl WindowManager {
    pub fn focused(&self) -> Option<WindowId> {
        self.focus_history.last().copied()
    }

    pub fn mode(&self, window: WindowId) -> Option<WindowMode> {
        self.windows.get(&window).map(|state| state.mode)
    }

    pub fn contains(&self, window: WindowId) -> bool {
        self.windows.contains_key(&window)
    }

    pub fn window_ids(&self) -> impl Iterator<Item = WindowId> + '_ {
        self.windows.keys().copied()
    }

    pub(crate) fn recommended_tiled_anchor(&self) -> Option<WindowId> {
        self.focus_history
            .iter()
            .rev()
            .copied()
            .find(|window| self.mode(*window) == Some(WindowMode::Tiled))
    }

    pub fn insert_tiled(
        &mut self,
        window: WindowId,
        anchor: Option<WindowId>,
        axis: SplitAxis,
    ) -> Result<(), WindowManagerError> {
        if self.contains(window) {
            return Err(WindowManagerError::DuplicateWindow(window));
        }
        self.layout.insert_after(anchor, window, axis)?;
        self.windows.insert(
            window,
            WindowState {
                mode: WindowMode::Tiled,
                floating_rect: None,
            },
        );
        self.focus(window)?;
        Ok(())
    }

    pub fn insert_floating(
        &mut self,
        window: WindowId,
        rect: Rect,
    ) -> Result<(), WindowManagerError> {
        if self.contains(window) {
            return Err(WindowManagerError::DuplicateWindow(window));
        }
        self.windows.insert(
            window,
            WindowState {
                mode: WindowMode::Floating,
                floating_rect: Some(rect),
            },
        );
        self.floating_order.push(window);
        self.focus(window)?;
        Ok(())
    }

    pub fn focus(&mut self, window: WindowId) -> Result<(), WindowManagerError> {
        if !self.contains(window) {
            return Err(WindowManagerError::UnknownWindow(window));
        }
        self.focus_history.retain(|focused| *focused != window);
        self.focus_history.push(window);
        if self.mode(window) == Some(WindowMode::Floating) {
            self.floating_order.retain(|floating| *floating != window);
            self.floating_order.push(window);
        }
        Ok(())
    }

    pub fn close(&mut self, window: WindowId) -> Result<(), WindowManagerError> {
        let state = *self
            .windows
            .get(&window)
            .ok_or(WindowManagerError::UnknownWindow(window))?;
        if state.mode == WindowMode::Tiled {
            self.layout.remove(window)?;
        }
        self.windows.remove(&window);
        self.floating_order.retain(|floating| *floating != window);
        self.focus_history.retain(|focused| *focused != window);
        Ok(())
    }

    pub(crate) fn take_for_transfer(
        &mut self,
        window: WindowId,
    ) -> Result<WindowSnapshot, WindowManagerError> {
        let state = *self
            .windows
            .get(&window)
            .ok_or(WindowManagerError::UnknownWindow(window))?;
        if state.mode == WindowMode::Tiled {
            self.layout.remove(window)?;
        }
        self.windows.remove(&window);
        self.floating_order.retain(|floating| *floating != window);
        self.focus_history.retain(|focused| *focused != window);
        Ok(WindowSnapshot {
            id: window,
            mode: state.mode,
            floating_rect: state.floating_rect,
        })
    }

    pub(crate) fn insert_transferred(
        &mut self,
        snapshot: WindowSnapshot,
        axis: SplitAxis,
    ) -> Result<(), WindowManagerError> {
        if self.contains(snapshot.id) {
            return Err(WindowManagerError::DuplicateWindow(snapshot.id));
        }
        match snapshot.mode {
            WindowMode::Tiled => {
                self.layout
                    .insert_after(self.recommended_tiled_anchor(), snapshot.id, axis)?;
            }
            WindowMode::Floating if snapshot.floating_rect.is_none() => {
                return Err(WindowManagerError::FloatingGeometryRequired);
            }
            WindowMode::Floating => {}
        }
        self.windows.insert(
            snapshot.id,
            WindowState {
                mode: snapshot.mode,
                floating_rect: snapshot.floating_rect,
            },
        );
        if snapshot.mode == WindowMode::Floating {
            self.floating_order.push(snapshot.id);
        }
        self.focus(snapshot.id)?;
        Ok(())
    }

    /// Change tiling mode. Floating geometry is required when moving into floating mode.
    pub fn set_mode(
        &mut self,
        window: WindowId,
        mode: WindowMode,
        floating_rect: Option<Rect>,
        axis: SplitAxis,
    ) -> Result<(), WindowManagerError> {
        let current = *self
            .windows
            .get(&window)
            .ok_or(WindowManagerError::UnknownWindow(window))?;
        if current.mode == mode {
            if mode == WindowMode::Floating {
                if let Some(rect) = floating_rect {
                    let state = self
                        .windows
                        .get_mut(&window)
                        .ok_or(WindowManagerError::UnknownWindow(window))?;
                    state.floating_rect = Some(rect);
                }
            }
            return Ok(());
        }

        match mode {
            WindowMode::Floating => {
                let rect = floating_rect.ok_or(WindowManagerError::FloatingGeometryRequired)?;
                self.layout.remove(window)?;
                let state = self
                    .windows
                    .get_mut(&window)
                    .ok_or(WindowManagerError::UnknownWindow(window))?;
                state.mode = WindowMode::Floating;
                state.floating_rect = Some(rect);
                self.floating_order.push(window);
            }
            WindowMode::Tiled => {
                let anchor = self
                    .focus_history
                    .iter()
                    .rev()
                    .copied()
                    .find(|id| *id != window && self.mode(*id) == Some(WindowMode::Tiled));
                self.layout.insert_after(anchor, window, axis)?;
                self.windows
                    .get_mut(&window)
                    .ok_or(WindowManagerError::UnknownWindow(window))?
                    .mode = WindowMode::Tiled;
                self.floating_order.retain(|floating| *floating != window);
            }
        }
        self.focus(window)?;
        Ok(())
    }

    pub fn set_floating_rect(
        &mut self,
        window: WindowId,
        rect: Rect,
    ) -> Result<(), WindowManagerError> {
        let state = self
            .windows
            .get_mut(&window)
            .ok_or(WindowManagerError::UnknownWindow(window))?;
        if state.mode != WindowMode::Floating {
            return Err(WindowManagerError::NotFloating(window));
        }
        state.floating_rect = Some(rect);
        Ok(())
    }

    pub fn set_split_ratio(
        &mut self,
        window: WindowId,
        ratio_basis_points: u16,
    ) -> Result<(), WindowManagerError> {
        if !self.contains(window) {
            return Err(WindowManagerError::UnknownWindow(window));
        }
        if self.mode(window) != Some(WindowMode::Tiled) {
            return Err(WindowManagerError::NotTiled(window));
        }
        self.layout
            .set_split_ratio(window, ratio_basis_points)
            .map_err(Into::into)
    }

    pub fn arrange(
        &self,
        bounds: Rect,
        minimum: Size,
    ) -> Result<Vec<ManagedPlacement>, WindowManagerError> {
        let tiled = self.layout.arrange(bounds, minimum)?;
        let mut result = Vec::with_capacity(self.windows.len());
        result.extend(
            tiled
                .into_iter()
                .map(|Placement { window, rect }| ManagedPlacement {
                    window,
                    rect,
                    mode: WindowMode::Tiled,
                }),
        );

        for window in &self.floating_order {
            let Some(state) = self.windows.get(window) else {
                continue;
            };
            let Some(rect) = state.floating_rect else {
                continue;
            };
            result.push(ManagedPlacement {
                window: *window,
                rect: clamp_floating_rect(rect, bounds, minimum)?,
                mode: WindowMode::Floating,
            });
        }
        Ok(result)
    }
}

fn clamp_floating_rect(
    rect: Rect,
    bounds: Rect,
    minimum: Size,
) -> Result<Rect, WindowManagerError> {
    if bounds.width < minimum.width || bounds.height < minimum.height {
        return Err(WindowManagerError::Layout(LayoutError::InsufficientSpace {
            available: Size {
                width: bounds.width,
                height: bounds.height,
            },
            required: minimum,
        }));
    }

    let width = rect.width.clamp(minimum.width, bounds.width);
    let height = rect.height.clamp(minimum.height, bounds.height);
    let max_x = bounds
        .x
        .checked_add(
            i32::try_from(bounds.width - width)
                .map_err(|_| WindowManagerError::Layout(LayoutError::CoordinateOverflow))?,
        )
        .ok_or(WindowManagerError::Layout(LayoutError::CoordinateOverflow))?;
    let max_y = bounds
        .y
        .checked_add(
            i32::try_from(bounds.height - height)
                .map_err(|_| WindowManagerError::Layout(LayoutError::CoordinateOverflow))?,
        )
        .ok_or(WindowManagerError::Layout(LayoutError::CoordinateOverflow))?;
    Ok(Rect {
        x: rect.x.clamp(bounds.x, max_x),
        y: rect.y.clamp(bounds.y, max_y),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

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
    fn inserts_and_arranges_tiled_windows() {
        let mut manager = WindowManager::default();
        manager
            .insert_tiled(id(1), None, SplitAxis::Horizontal)
            .unwrap();
        manager
            .insert_tiled(id(2), Some(id(1)), SplitAxis::Horizontal)
            .unwrap();

        let placements = manager.arrange(rect(0, 0, 100, 60), MINIMUM).unwrap();
        assert_eq!(placements[0].window, id(1));
        assert_eq!(placements[0].rect, rect(0, 0, 50, 60));
        assert_eq!(placements[1].rect, rect(50, 0, 50, 60));
        assert_eq!(manager.focused(), Some(id(2)));
    }

    #[test]
    fn floating_window_is_clamped_and_raised_on_focus() {
        let mut manager = WindowManager::default();
        manager
            .insert_floating(id(1), rect(-50, 100, 400, 20))
            .unwrap();
        manager
            .insert_floating(id(2), rect(20, 20, 30, 30))
            .unwrap();
        manager.focus(id(1)).unwrap();

        let placements = manager.arrange(rect(0, 0, 200, 120), MINIMUM).unwrap();
        assert_eq!(placements[0].window, id(2));
        assert_eq!(placements[1].window, id(1));
        assert_eq!(placements[1].rect, rect(0, 100, 200, 20));
    }

    #[test]
    fn switches_between_tiled_and_floating_modes() {
        let mut manager = WindowManager::default();
        manager
            .insert_tiled(id(1), None, SplitAxis::Horizontal)
            .unwrap();
        manager
            .insert_tiled(id(2), Some(id(1)), SplitAxis::Horizontal)
            .unwrap();
        manager
            .set_mode(
                id(1),
                WindowMode::Floating,
                Some(rect(5, 5, 25, 25)),
                SplitAxis::Horizontal,
            )
            .unwrap();
        assert_eq!(manager.mode(id(1)), Some(WindowMode::Floating));
        assert_eq!(
            manager.arrange(rect(0, 0, 100, 60), MINIMUM).unwrap().len(),
            2
        );

        manager
            .set_mode(id(1), WindowMode::Tiled, None, SplitAxis::Vertical)
            .unwrap();
        assert_eq!(manager.mode(id(1)), Some(WindowMode::Tiled));
        assert_eq!(
            manager.arrange(rect(0, 0, 100, 60), MINIMUM).unwrap().len(),
            2
        );
    }

    #[test]
    fn closing_focused_window_restores_previous_focus() {
        let mut manager = WindowManager::default();
        manager
            .insert_tiled(id(1), None, SplitAxis::Horizontal)
            .unwrap();
        manager
            .insert_tiled(id(2), Some(id(1)), SplitAxis::Horizontal)
            .unwrap();
        manager.focus(id(1)).unwrap();
        manager.close(id(1)).unwrap();

        assert_eq!(manager.focused(), Some(id(2)));
        assert!(!manager.contains(id(1)));
    }

    #[test]
    fn focus_and_mode_changes_reject_unknown_windows() {
        let mut manager = WindowManager::default();
        assert_eq!(
            manager.focus(id(1)),
            Err(WindowManagerError::UnknownWindow(id(1)))
        );
        assert_eq!(
            manager.set_mode(id(1), WindowMode::Tiled, None, SplitAxis::Horizontal),
            Err(WindowManagerError::UnknownWindow(id(1)))
        );
    }

    #[test]
    fn switching_to_floating_requires_geometry_and_keeps_state_on_error() {
        let mut manager = WindowManager::default();
        manager
            .insert_tiled(id(1), None, SplitAxis::Horizontal)
            .unwrap();

        assert_eq!(
            manager.set_mode(id(1), WindowMode::Floating, None, SplitAxis::Horizontal),
            Err(WindowManagerError::FloatingGeometryRequired)
        );
        assert_eq!(manager.mode(id(1)), Some(WindowMode::Tiled));
        assert_eq!(
            manager.arrange(rect(0, 0, 80, 60), MINIMUM).unwrap().len(),
            1
        );
    }
}
