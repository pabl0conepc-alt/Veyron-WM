use std::{
    collections::{BTreeMap, VecDeque},
    num::NonZeroU64,
};

use super::{
    layout::{SplitAxis, WindowId},
    manager::{WindowManager, WindowManagerError},
};

const MAX_NAVIGATION_HISTORY: usize = 64;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WorkspaceId(NonZeroU64);

impl WorkspaceId {
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopError {
    DuplicateWindow(WindowId),
    UnknownWindow(WindowId),
    UnknownWorkspace(Position),
    PositionOverflow,
    WorkspaceIdExhausted,
    WindowManager(WindowManagerError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Workspace {
    id: WorkspaceId,
    windows: WindowManager,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpatialDesktop {
    workspaces: BTreeMap<Position, Workspace>,
    active: Position,
    next_id: Option<NonZeroU64>,
    history: VecDeque<Position>,
}

impl Default for SpatialDesktop {
    fn default() -> Self {
        let origin = Position { x: 0, y: 0 };
        let mut workspaces = BTreeMap::new();
        workspaces.insert(
            origin,
            Workspace {
                id: WorkspaceId(NonZeroU64::MIN),
                windows: WindowManager::default(),
            },
        );
        Self {
            workspaces,
            active: origin,
            next_id: NonZeroU64::new(2),
            history: VecDeque::new(),
        }
    }
}

impl SpatialDesktop {
    pub fn active_position(&self) -> Position {
        self.active
    }

    pub fn active_workspace_id(&self) -> WorkspaceId {
        self.workspaces[&self.active].id
    }

    pub fn workspace_count(&self) -> usize {
        self.workspaces.len()
    }

    pub fn positions(&self) -> impl Iterator<Item = Position> + '_ {
        self.workspaces.keys().copied()
    }

    pub fn workspace_id_at(&self, position: Position) -> Option<WorkspaceId> {
        self.workspaces.get(&position).map(|workspace| workspace.id)
    }

    pub fn windows_at(&self, position: Position) -> Option<Vec<WindowId>> {
        self.workspaces
            .get(&position)
            .map(|workspace| workspace.windows.window_ids().collect())
    }

    pub fn active_window_manager(&self) -> Option<&WindowManager> {
        self.workspaces
            .get(&self.active)
            .map(|workspace| &workspace.windows)
    }

    pub fn active_window_manager_mut(&mut self) -> Option<&mut WindowManager> {
        self.workspaces
            .get_mut(&self.active)
            .map(|workspace| &mut workspace.windows)
    }

    pub fn window_manager_at(&self, position: Position) -> Option<&WindowManager> {
        self.workspaces
            .get(&position)
            .map(|workspace| &workspace.windows)
    }

    /// Move one grid step, creating the destination workspace only when first visited.
    pub fn navigate(&mut self, direction: Direction) -> Result<WorkspaceId, DesktopError> {
        let target = match direction {
            Direction::Left => Position {
                x: self
                    .active
                    .x
                    .checked_sub(1)
                    .ok_or(DesktopError::PositionOverflow)?,
                y: self.active.y,
            },
            Direction::Right => Position {
                x: self
                    .active
                    .x
                    .checked_add(1)
                    .ok_or(DesktopError::PositionOverflow)?,
                y: self.active.y,
            },
            Direction::Up => Position {
                x: self.active.x,
                y: self
                    .active
                    .y
                    .checked_sub(1)
                    .ok_or(DesktopError::PositionOverflow)?,
            },
            Direction::Down => Position {
                x: self.active.x,
                y: self
                    .active
                    .y
                    .checked_add(1)
                    .ok_or(DesktopError::PositionOverflow)?,
            },
        };
        self.activate_or_create(target)
    }

    pub fn switch_to(&mut self, position: Position) -> Result<WorkspaceId, DesktopError> {
        if !self.workspaces.contains_key(&position) {
            return Err(DesktopError::UnknownWorkspace(position));
        }
        self.activate(position);
        Ok(self.active_workspace_id())
    }

    pub fn navigate_back(&mut self) -> Option<WorkspaceId> {
        while let Some(position) = self.history.pop_back() {
            if position != self.active && self.workspaces.contains_key(&position) {
                self.active = position;
                return Some(self.active_workspace_id());
            }
        }
        None
    }

    pub fn add_window(&mut self, window: WindowId) -> Result<(), DesktopError> {
        if self.contains_window(window) {
            return Err(DesktopError::DuplicateWindow(window));
        }
        let workspace = self
            .workspaces
            .get_mut(&self.active)
            .ok_or(DesktopError::UnknownWorkspace(self.active))?;
        let anchor = workspace.windows.recommended_tiled_anchor();
        workspace
            .windows
            .insert_tiled(window, anchor, SplitAxis::Horizontal)
            .map_err(DesktopError::WindowManager)?;
        Ok(())
    }

    pub fn move_window_to(
        &mut self,
        window: WindowId,
        destination: Position,
    ) -> Result<(), DesktopError> {
        let source = self
            .workspaces
            .iter()
            .find_map(|(position, workspace)| {
                workspace.windows.contains(window).then_some(*position)
            })
            .ok_or(DesktopError::UnknownWindow(window))?;
        if source == destination {
            return Ok(());
        }

        self.ensure_workspace(destination)?;
        let snapshot = self
            .workspaces
            .get_mut(&source)
            .ok_or(DesktopError::UnknownWorkspace(source))?
            .windows
            .take_for_transfer(window)
            .map_err(DesktopError::WindowManager)?;
        self.workspaces
            .get_mut(&destination)
            .ok_or(DesktopError::UnknownWorkspace(destination))?
            .windows
            .insert_transferred(snapshot, SplitAxis::Horizontal)
            .map_err(DesktopError::WindowManager)?;
        Ok(())
    }

    pub fn close_window(&mut self, window: WindowId) -> Result<(), DesktopError> {
        for workspace in self.workspaces.values_mut() {
            if workspace.windows.contains(window) {
                return workspace
                    .windows
                    .close(window)
                    .map_err(DesktopError::WindowManager);
            }
        }
        Err(DesktopError::UnknownWindow(window))
    }

    fn contains_window(&self, window: WindowId) -> bool {
        self.workspaces
            .values()
            .any(|workspace| workspace.windows.contains(window))
    }

    fn activate_or_create(&mut self, position: Position) -> Result<WorkspaceId, DesktopError> {
        self.ensure_workspace(position)?;
        self.activate(position);
        Ok(self.active_workspace_id())
    }

    fn activate(&mut self, position: Position) {
        if self.active != position {
            if self.history.back().copied() != Some(self.active) {
                if self.history.len() == MAX_NAVIGATION_HISTORY {
                    self.history.pop_front();
                }
                self.history.push_back(self.active);
            }
            self.active = position;
        }
    }

    fn ensure_workspace(&mut self, position: Position) -> Result<WorkspaceId, DesktopError> {
        if let Some(workspace) = self.workspaces.get(&position) {
            return Ok(workspace.id);
        }
        let id = self
            .next_id
            .take()
            .map(WorkspaceId)
            .ok_or(DesktopError::WorkspaceIdExhausted)?;
        self.next_id = id.0.get().checked_add(1).and_then(NonZeroU64::new);
        self.workspaces.insert(
            position,
            Workspace {
                id,
                windows: WindowManager::default(),
            },
        );
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU64;

    fn window(value: u64) -> WindowId {
        WindowId::new(NonZeroU64::new(value).expect("test ids are nonzero"))
    }

    fn position(x: i32, y: i32) -> Position {
        Position { x, y }
    }

    #[test]
    fn creates_spaces_lazily_in_each_direction() {
        let mut desktop = SpatialDesktop::default();
        let origin_id = desktop.active_workspace_id();

        let right = desktop.navigate(Direction::Right).unwrap();
        let above = desktop.navigate(Direction::Up).unwrap();
        let left = desktop.navigate(Direction::Left).unwrap();
        let below = desktop.navigate(Direction::Down).unwrap();

        assert_ne!(origin_id, right);
        assert_ne!(right, above);
        assert_eq!(desktop.active_position(), position(0, 0));
        assert_eq!(desktop.workspace_count(), 4);
        assert_ne!(left, below);
    }

    #[test]
    fn revisiting_position_keeps_workspace_identity_and_contents() {
        let mut desktop = SpatialDesktop::default();
        desktop.add_window(window(1)).unwrap();
        let origin = desktop.active_workspace_id();
        desktop.navigate(Direction::Right).unwrap();
        desktop.navigate(Direction::Left).unwrap();

        assert_eq!(desktop.active_workspace_id(), origin);
        assert_eq!(desktop.windows_at(position(0, 0)), Some(vec![window(1)]));
        assert_eq!(desktop.workspace_count(), 2);
    }

    #[test]
    fn moving_a_window_preserves_its_id_and_leaves_active_space_unchanged() {
        let mut desktop = SpatialDesktop::default();
        desktop.add_window(window(1)).unwrap();
        let origin = desktop.active_position();
        let target = position(20, -4);
        desktop.move_window_to(window(1), target).unwrap();

        assert_eq!(desktop.active_position(), origin);
        assert_eq!(desktop.windows_at(origin), Some(Vec::new()));
        assert_eq!(desktop.windows_at(target), Some(vec![window(1)]));
        assert!(
            desktop
                .window_manager_at(target)
                .unwrap()
                .contains(window(1))
        );
        assert_eq!(desktop.workspace_count(), 2);
    }

    #[test]
    fn moving_floating_window_preserves_mode_and_geometry() {
        let mut desktop = SpatialDesktop::default();
        desktop
            .active_window_manager_mut()
            .unwrap()
            .insert_floating(
                window(1),
                super::super::layout::Rect {
                    x: 15,
                    y: 20,
                    width: 40,
                    height: 30,
                },
            )
            .unwrap();
        let target = position(-3, 5);
        desktop.move_window_to(window(1), target).unwrap();

        let manager = desktop.window_manager_at(target).unwrap();
        assert_eq!(
            manager.mode(window(1)),
            Some(super::super::manager::WindowMode::Floating)
        );
        assert_eq!(
            manager
                .arrange(
                    super::super::layout::Rect {
                        x: 0,
                        y: 0,
                        width: 100,
                        height: 80,
                    },
                    super::super::layout::Size {
                        width: 10,
                        height: 10,
                    },
                )
                .unwrap()[0]
                .rect,
            super::super::layout::Rect {
                x: 15,
                y: 20,
                width: 40,
                height: 30,
            }
        );
    }

    #[test]
    fn navigation_history_returns_to_previous_workspace() {
        let mut desktop = SpatialDesktop::default();
        let origin = desktop.active_workspace_id();
        desktop.navigate(Direction::Right).unwrap();
        desktop.navigate(Direction::Down).unwrap();

        assert_eq!(
            desktop.navigate_back(),
            Some(desktop.workspace_id_at(position(1, 0)).unwrap())
        );
        assert_eq!(desktop.navigate_back(), Some(origin));
        assert_eq!(desktop.navigate_back(), None);
    }

    #[test]
    fn navigation_history_is_bounded() {
        let mut desktop = SpatialDesktop::default();
        for _ in 0..100 {
            desktop.navigate(Direction::Right).unwrap();
        }

        assert_eq!(desktop.history.len(), MAX_NAVIGATION_HISTORY);
        for _ in 0..MAX_NAVIGATION_HISTORY {
            desktop.navigate_back().unwrap();
        }
        assert_eq!(desktop.active_position(), position(36, 0));
    }

    #[test]
    fn rejects_duplicate_or_unknown_window_operations() {
        let mut desktop = SpatialDesktop::default();
        desktop.add_window(window(1)).unwrap();
        assert_eq!(
            desktop.add_window(window(1)),
            Err(DesktopError::DuplicateWindow(window(1)))
        );
        assert_eq!(
            desktop.close_window(window(2)),
            Err(DesktopError::UnknownWindow(window(2)))
        );
        desktop.close_window(window(1)).unwrap();
        assert_eq!(desktop.windows_at(position(0, 0)), Some(Vec::new()));
    }

    #[test]
    fn direct_switch_does_not_allocate_an_unknown_space() {
        let mut desktop = SpatialDesktop::default();
        let unknown = position(10, 10);
        assert_eq!(
            desktop.switch_to(unknown),
            Err(DesktopError::UnknownWorkspace(unknown))
        );
        assert_eq!(desktop.workspace_count(), 1);
    }

    #[test]
    fn coordinate_overflow_does_not_change_active_position() {
        let mut desktop = SpatialDesktop::default();
        desktop.active = position(i32::MAX, 0);
        let before = desktop.active_position();

        assert_eq!(
            desktop.navigate(Direction::Right),
            Err(DesktopError::PositionOverflow)
        );
        assert_eq!(desktop.active_position(), before);
    }
}
