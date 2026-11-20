//! The window manager state machine.
//!
//! [`Wm`] owns outputs, workspaces, window metadata and the tiling layout. It is
//! a pure state machine: it takes protocol-neutral inputs (windows appearing or
//! disappearing, [`Action`]s) and produces a [`Snapshot`] describing exactly
//! where every window must be drawn. The compositor is responsible for turning
//! that snapshot into Wayland configures, input focus and rendering.
//!
//! Keeping this split strict means window management policy can be unit tested
//! with no GPU, no display server and no Wayland clients.

use std::collections::HashMap;

use crate::{
    action::Action,
    config::Config,
    geometry::{Rect, Size},
    layout::{default_floating_size, viewport_for, Placement, ScrollLayout},
    window::{NewWindow, Window, WindowId},
};

/// A workspace is one scrollable strip plus its floating layer.
#[derive(Debug, Clone)]
pub struct Workspace {
    name: String,
    position: WorkspacePosition,
    layout: ScrollLayout,
    floating: Vec<WindowId>,
    focused_floating: Option<WindowId>,
    fullscreen: Option<WindowId>,
}

impl Workspace {
    fn new(
        name: String,
        position: WorkspacePosition,
        config: &crate::layout::LayoutConfig,
        viewport: Rect,
    ) -> Self {
        Self {
            name,
            position,
            layout: ScrollLayout::new(config.clone(), viewport),
            floating: Vec::new(),
            focused_floating: None,
            fullscreen: None,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn position(&self) -> WorkspacePosition {
        self.position
    }

    pub fn layout(&self) -> &ScrollLayout {
        &self.layout
    }

    pub fn floating_windows(&self) -> &[WindowId] {
        &self.floating
    }

    pub fn fullscreen_window(&self) -> Option<WindowId> {
        self.fullscreen
    }

    /// The focused window of this workspace, taking fullscreen and floating
    /// layers into account.
    pub fn focused_window(&self) -> Option<WindowId> {
        self.fullscreen
            .or(self.focused_floating)
            .or_else(|| self.layout.focused_window())
    }
}

/// Sparse signed coordinates for an infinite desktop workspace.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct WorkspacePosition {
    pub x: i64,
    pub y: i64,
}

impl WorkspacePosition {
    fn offset(self, direction: crate::action::Direction) -> Option<Self> {
        use crate::action::Direction;
        let (dx, dy) = match direction {
            Direction::Left => (-1, 0),
            Direction::Right => (1, 0),
            Direction::Up => (0, -1),
            Direction::Down => (0, 1),
        };
        Some(Self {
            x: self.x.checked_add(dx)?,
            y: self.y.checked_add(dy)?,
        })
    }
}

/// One output (monitor) and the workspaces it shows.
#[derive(Debug, Clone)]
pub struct OutputState {
    name: String,
    rect: Rect,
    workspaces: Vec<Workspace>,
    workspace_indices: HashMap<WorkspacePosition, usize>,
    active: usize,
}

impl OutputState {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn rect(&self) -> Rect {
        self.rect
    }

    pub fn workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }

    pub fn active_workspace_index(&self) -> usize {
        self.active
    }

    pub fn active_workspace_position(&self) -> WorkspacePosition {
        self.active_workspace().position
    }

    pub fn active_workspace(&self) -> &Workspace {
        &self.workspaces[self.active]
    }
}

/// Side effects the compositor has to carry out after an action.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Effects {
    /// Window geometry or stacking may have changed.
    pub layout_changed: bool,
    /// Keyboard focus must be moved.
    pub focus_changed: bool,
    /// The visible workspace changed.
    pub workspace_changed: bool,
    /// The configuration should be re-read from disk.
    pub reload_config: bool,
    /// The compositor should exit.
    pub quit: bool,
    /// Ask this window's client to close; removal happens when the client
    /// destroys the surface.
    pub close_requested: Option<WindowId>,
    /// Shell commands to run.
    pub spawn: Vec<String>,
}

impl Effects {
    /// Whether the frame needs to be redrawn.
    pub fn needs_redraw(&self) -> bool {
        self.layout_changed || self.focus_changed || self.workspace_changed
    }
}

/// Where a window is placed in the current frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlacementInfo {
    pub window: WindowId,
    pub rect: Rect,
    pub floating: bool,
    pub fullscreen: bool,
}

/// Everything the compositor needs to draw one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// Full output rectangle, used to clear the frame.
    pub output: Rect,
    /// Usable area inside the configured outer margin.
    pub viewport: Rect,
    pub workspace: String,
    pub workspace_index: usize,
    pub workspace_count: usize,
    /// Signed 2D coordinate on the sparse infinite desktop.
    pub workspace_position: WorkspacePosition,
    pub columns: usize,
    pub scroll: f64,
    pub focused: Option<WindowId>,
    /// Tiled windows in strip order, then floating windows, then fullscreen.
    pub placements: Vec<PlacementInfo>,
}

impl Snapshot {
    pub fn placement_of(&self, window: WindowId) -> Option<PlacementInfo> {
        self.placements
            .iter()
            .find(|placement| placement.window == window)
            .copied()
    }

    /// Windows visible on the current workspace, in draw order.
    pub fn windows(&self) -> impl Iterator<Item = WindowId> + '_ {
        self.placements.iter().map(|placement| placement.window)
    }

    pub fn is_floating(&self, window: WindowId) -> bool {
        self.placement_of(window)
            .map(|placement| placement.floating)
            .unwrap_or(false)
    }

    pub fn is_fullscreen(&self, window: WindowId) -> bool {
        self.placement_of(window)
            .map(|placement| placement.fullscreen)
            .unwrap_or(false)
    }
}

/// The Veyron window manager.
#[derive(Debug)]
pub struct Wm {
    config: Config,
    windows: HashMap<WindowId, Window>,
    outputs: Vec<OutputState>,
    active_output: usize,
    next_id: u64,
}

impl Wm {
    /// Create a window manager with a single output and a single workspace.
    pub fn new(config: Config, output_name: impl Into<String>, size: Size) -> Self {
        let rect = Rect::from_size(size);
        let viewport = viewport_for(rect, &config.layout);
        let workspace = Workspace::new(
            "1".to_string(),
            WorkspacePosition::default(),
            &config.layout,
            viewport,
        );
        let workspace_indices = HashMap::from([(WorkspacePosition::default(), 0)]);
        Self {
            config,
            windows: HashMap::new(),
            outputs: vec![OutputState {
                name: output_name.into(),
                rect,
                workspaces: vec![workspace],
                workspace_indices,
                active: 0,
            }],
            active_output: 0,
            next_id: 0,
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Apply a new configuration, keeping window and workspace state.
    pub fn set_config(&mut self, config: Config) {
        self.config = config;
        let viewport = self.viewport();
        for output in &mut self.outputs {
            for workspace in &mut output.workspaces {
                workspace.layout.set_config(self.config.layout.clone());
                workspace.layout.set_viewport(viewport);
            }
        }
        self.clamp_floating_windows();
        self.refresh_tiled_rects();
    }

    pub fn outputs(&self) -> &[OutputState] {
        &self.outputs
    }

    pub fn active_output(&self) -> &OutputState {
        &self.outputs[self.active_output]
    }

    pub fn output_rect(&self) -> Rect {
        self.active_output().rect
    }

    pub fn viewport(&self) -> Rect {
        viewport_for(self.active_output().rect, &self.config.layout)
    }

    pub fn window(&self, id: WindowId) -> Option<&Window> {
        self.windows.get(&id)
    }

    pub fn windows(&self) -> impl Iterator<Item = &Window> {
        self.windows.values()
    }

    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    pub fn workspace_count(&self) -> usize {
        self.active_output().workspaces.len()
    }

    pub fn focused_window(&self) -> Option<WindowId> {
        self.active_output().active_workspace().focused_window()
    }

    /// Create a managed window and place it on the active workspace.
    pub fn create_window(&mut self, request: NewWindow) -> WindowId {
        self.next_id += 1;
        let id = WindowId(self.next_id);

        let mut window = Window::new(id, request);
        let viewport = self.viewport();
        let floating_size = window
            .min_size()
            .map(|size| size.clamp_to(viewport.size()))
            .unwrap_or_else(|| default_floating_size(viewport));
        window.set_floating_rect(viewport.centered(floating_size));
        self.windows.insert(id, window);

        let workspace = self.active_workspace_mut();
        workspace.focused_floating = None;
        workspace.layout.insert_window(id);

        self.refresh_tiled_rects();
        id
    }

    /// Forget a window. Windows disappear when their surface is destroyed,
    /// including after a close request, so this is the single removal path.
    pub fn remove_window(&mut self, id: WindowId) -> bool {
        if self.windows.remove(&id).is_none() {
            return false;
        }
        for output in &mut self.outputs {
            for workspace in &mut output.workspaces {
                if workspace.fullscreen == Some(id) {
                    workspace.fullscreen = None;
                }
                if workspace.focused_floating == Some(id) {
                    workspace.focused_floating = None;
                }
                workspace.floating.retain(|window| *window != id);
                workspace.layout.remove_window(id);
            }
        }
        self.refresh_tiled_rects();
        true
    }

    pub fn set_window_title(&mut self, id: WindowId, title: impl Into<String>) {
        if let Some(window) = self.windows.get_mut(&id) {
            window.set_title(title.into());
        }
    }

    pub fn set_window_app_id(&mut self, id: WindowId, app_id: impl Into<String>) {
        if let Some(window) = self.windows.get_mut(&id) {
            window.set_app_id(app_id.into());
        }
    }

    /// Focus a window, activating its workspace and output if necessary.
    /// Returns whether the focus target changed.
    pub fn focus_window(&mut self, id: WindowId) -> bool {
        if !self.windows.contains_key(&id) {
            return false;
        }
        let Some((output_index, workspace_index, floating)) = self.locate(id) else {
            return false;
        };

        let previous = self.focused_window();
        self.active_output = output_index;
        self.outputs[output_index].active = workspace_index;

        let workspace = &mut self.outputs[output_index].workspaces[workspace_index];
        if floating {
            workspace.floating.retain(|window| *window != id);
            workspace.floating.push(id);
            workspace.focused_floating = Some(id);
        } else {
            workspace.focused_floating = None;
            workspace.layout.focus_window(id);
        }

        previous != self.focused_window()
    }

    /// Dispatch an action, returning the side effects to carry out.
    pub fn dispatch(&mut self, action: Action) -> Effects {
        let mut effects = Effects::default();

        match action {
            Action::Close => effects.close_requested = self.focused_window(),
            Action::Quit => effects.quit = true,
            Action::ReloadConfig => effects.reload_config = true,
            Action::Spawn { command } => effects.spawn.push(command),
            Action::Focus { direction } => {
                let workspace = self.active_workspace_mut();
                workspace.focused_floating = None;
                effects.focus_changed = workspace.layout.focus_direction(direction);
                effects.layout_changed = effects.focus_changed;
            }
            Action::Move { direction } => {
                let workspace = self.active_workspace_mut();
                workspace.focused_floating = None;
                effects.layout_changed = workspace.layout.move_window(direction);
            }
            Action::ColumnWidth { delta } => {
                effects.layout_changed = self.active_workspace_mut().layout.resize_focused(delta);
            }
            Action::CenterColumn => {
                effects.layout_changed = self.active_workspace_mut().layout.center_focus();
            }
            Action::ToggleFullscreen => effects.layout_changed = self.toggle_fullscreen(),
            Action::ToggleFloating => effects.layout_changed = self.toggle_floating(),
            Action::Workspace { index } => {
                effects.workspace_changed = self.switch_workspace(index.saturating_sub(1));
                effects.layout_changed = effects.workspace_changed;
                effects.focus_changed = effects.workspace_changed;
            }
            Action::WorkspaceNavigate { direction } => {
                effects.workspace_changed = self.navigate_workspace(direction);
                effects.layout_changed = effects.workspace_changed;
                effects.focus_changed = effects.workspace_changed;
            }
            Action::MoveToWorkspace { direction } => {
                effects.workspace_changed = self.move_focused_to_workspace(direction);
                effects.layout_changed = effects.workspace_changed;
                effects.focus_changed = effects.workspace_changed;
            }
            Action::WorkspaceNext => {
                effects.workspace_changed =
                    self.navigate_workspace(crate::action::Direction::Right);
                effects.layout_changed = effects.workspace_changed;
                effects.focus_changed = effects.workspace_changed;
            }
            Action::WorkspacePrev => {
                effects.workspace_changed = self.navigate_workspace(crate::action::Direction::Left);
                effects.layout_changed = effects.workspace_changed;
                effects.focus_changed = effects.workspace_changed;
            }
        }

        if effects.layout_changed {
            self.refresh_tiled_rects();
        }
        effects
    }

    /// Update the output size (window resize in the nested backend, or a mode
    /// change on real hardware).
    pub fn set_output_size(&mut self, size: Size) {
        let rect = Rect::from_size(size);
        let viewport = viewport_for(rect, &self.config.layout);
        let output = &mut self.outputs[self.active_output];
        output.rect = rect;
        for workspace in &mut output.workspaces {
            workspace.layout.set_viewport(viewport);
        }
        self.clamp_floating_windows();
        self.refresh_tiled_rects();
    }

    /// Build the frame description the compositor renders.
    pub fn snapshot(&self) -> Snapshot {
        let output = self.active_output();
        let workspace = output.active_workspace();
        let viewport = viewport_for(output.rect, &self.config.layout);

        let mut placements = Vec::with_capacity(workspace.layout.window_count());

        match workspace.fullscreen {
            Some(id) if self.windows.contains_key(&id) => placements.push(PlacementInfo {
                window: id,
                rect: viewport,
                floating: self
                    .windows
                    .get(&id)
                    .map(Window::is_floating)
                    .unwrap_or(false),
                fullscreen: true,
            }),
            _ => {
                for Placement { window, rect } in workspace.layout.placements() {
                    placements.push(PlacementInfo {
                        window,
                        rect,
                        floating: false,
                        fullscreen: false,
                    });
                }
                for id in &workspace.floating {
                    if let Some(window) = self.windows.get(id) {
                        placements.push(PlacementInfo {
                            window: *id,
                            rect: window.floating_rect(),
                            floating: true,
                            fullscreen: false,
                        });
                    }
                }
            }
        }

        Snapshot {
            output: output.rect,
            viewport,
            workspace: workspace.name.clone(),
            workspace_index: output.active,
            workspace_count: output.workspaces.len(),
            workspace_position: workspace.position,
            columns: workspace.layout.column_count(),
            scroll: workspace.layout.scroll(),
            focused: workspace.focused_window(),
            placements,
        }
    }

    fn active_workspace_mut(&mut self) -> &mut Workspace {
        let output_index = self.active_output;
        let active = self.outputs[output_index].active;
        &mut self.outputs[output_index].workspaces[active]
    }

    fn locate(&self, id: WindowId) -> Option<(usize, usize, bool)> {
        for (output_index, output) in self.outputs.iter().enumerate() {
            for (workspace_index, workspace) in output.workspaces.iter().enumerate() {
                if workspace.layout.contains(id) {
                    return Some((output_index, workspace_index, false));
                }
                if workspace.floating.contains(&id) {
                    return Some((output_index, workspace_index, true));
                }
            }
        }
        None
    }

    fn switch_workspace(&mut self, index: usize) -> bool {
        let output_index = self.active_output;
        // Workspaces are created on demand: the strip is conceptually endless,
        // so there is no fixed maximum.
        while self.outputs[output_index].workspaces.len() <= index {
            let position = self.next_linear_position(output_index);
            self.append_workspace(output_index, position);
        }

        let output = &mut self.outputs[output_index];
        if output.active == index {
            return false;
        }
        output.active = index;
        true
    }

    fn next_linear_position(&self, output_index: usize) -> WorkspacePosition {
        let positions = &self.outputs[output_index].workspace_indices;
        let mut x = 0_i64;
        while positions.contains_key(&WorkspacePosition { x, y: 0 }) {
            let Some(next) = x.checked_add(1) else {
                return WorkspacePosition { x, y: 0 };
            };
            x = next;
        }
        WorkspacePosition { x, y: 0 }
    }

    fn append_workspace(&mut self, output_index: usize, position: WorkspacePosition) -> usize {
        let output = &mut self.outputs[output_index];
        let index = output.workspaces.len();
        let viewport = viewport_for(output.rect, &self.config.layout);
        output.workspaces.push(Workspace::new(
            (index + 1).to_string(),
            position,
            &self.config.layout,
            viewport,
        ));
        output.workspace_indices.insert(position, index);
        index
    }

    fn navigate_workspace(&mut self, direction: crate::action::Direction) -> bool {
        let output_index = self.active_output;
        let current = self.outputs[output_index].active_workspace_position();
        let Some(position) = current.offset(direction) else {
            return false;
        };
        self.switch_workspace_at(output_index, position)
    }

    fn switch_workspace_at(&mut self, output_index: usize, position: WorkspacePosition) -> bool {
        let index = self.outputs[output_index]
            .workspace_indices
            .get(&position)
            .copied()
            .unwrap_or_else(|| self.append_workspace(output_index, position));
        let output = &mut self.outputs[output_index];
        if output.active == index {
            return false;
        }
        output.active = index;
        true
    }

    fn move_focused_to_workspace(&mut self, direction: crate::action::Direction) -> bool {
        let Some(id) = self.focused_window() else {
            return false;
        };
        let output_index = self.active_output;
        let source_index = self.outputs[output_index].active;
        let source = &self.outputs[output_index].workspaces[source_index];
        let Some(destination_position) = source.position.offset(direction) else {
            return false;
        };
        if source.position == destination_position {
            return false;
        }

        let was_floating = source.floating.contains(&id);
        let was_fullscreen = source.fullscreen == Some(id);
        let occupied_fullscreen = self.outputs[output_index]
            .workspace_indices
            .get(&destination_position)
            .and_then(|index| self.outputs[output_index].workspaces[*index].fullscreen);
        if was_fullscreen && occupied_fullscreen.is_some() {
            return false;
        }
        let destination_index = self.outputs[output_index]
            .workspace_indices
            .get(&destination_position)
            .copied()
            .unwrap_or_else(|| self.append_workspace(output_index, destination_position));

        let output = &mut self.outputs[output_index];
        let source = &mut output.workspaces[source_index];
        source.layout.remove_window(id);
        source.floating.retain(|window| *window != id);
        if source.focused_floating == Some(id) {
            source.focused_floating = None;
        }
        if source.fullscreen == Some(id) {
            source.fullscreen = None;
        }

        let destination = &mut output.workspaces[destination_index];
        if was_floating {
            destination.floating.push(id);
            destination.focused_floating = Some(id);
        } else {
            destination.layout.insert_window(id);
        }
        if was_fullscreen {
            destination.fullscreen = Some(id);
        }
        output.active = destination_index;
        self.refresh_tiled_rects();
        true
    }

    fn toggle_fullscreen(&mut self) -> bool {
        let Some(id) = self.focused_window() else {
            return false;
        };
        let workspace = self.active_workspace_mut();
        if workspace.fullscreen == Some(id) {
            workspace.fullscreen = None;
        } else {
            workspace.fullscreen = Some(id);
            workspace.focused_floating = None;
        }
        true
    }

    fn toggle_floating(&mut self) -> bool {
        let Some(id) = self.focused_window() else {
            return false;
        };
        let is_floating = self
            .windows
            .get(&id)
            .map(Window::is_floating)
            .unwrap_or(false);

        if is_floating {
            if let Some(window) = self.windows.get_mut(&id) {
                window.set_floating(false);
            }
            let workspace = self.active_workspace_mut();
            workspace.floating.retain(|window| *window != id);
            workspace.focused_floating = None;
            workspace.layout.insert_window(id);
        } else {
            let viewport = self.viewport();
            let rect = self
                .windows
                .get(&id)
                .map(|window| {
                    let last = window.last_tiled_rect();
                    if last.is_empty() {
                        window.floating_rect()
                    } else {
                        last
                    }
                })
                .filter(|rect| !rect.is_empty())
                .unwrap_or_else(|| viewport.centered(default_floating_size(viewport)));

            let workspace = self.active_workspace_mut();
            workspace.layout.remove_window(id);
            workspace.focused_floating = Some(id);
            workspace.floating.push(id);
            if workspace.fullscreen == Some(id) {
                workspace.fullscreen = None;
            }

            if let Some(window) = self.windows.get_mut(&id) {
                window.set_floating(true);
                window.set_floating_rect(rect);
            }
        }
        true
    }

    /// Keep floating windows on screen after an output resize or margin change.
    fn clamp_floating_windows(&mut self) {
        let viewport = self.viewport();
        for output in &self.outputs {
            for workspace in &output.workspaces {
                for id in &workspace.floating {
                    if let Some(window) = self.windows.get_mut(id) {
                        let rect = window.floating_rect();
                        let size = rect.size().clamp_to(viewport.size());
                        let x = rect
                            .x
                            .clamp(viewport.x, (viewport.right() - size.w).max(viewport.x));
                        let y = rect
                            .y
                            .clamp(viewport.y, (viewport.bottom() - size.h).max(viewport.y));
                        window.set_floating_rect(Rect::new(x, y, size.w, size.h));
                    }
                }
            }
        }
    }

    /// Remember the last tiled geometry of every window so toggling floating
    /// off can restore a sensible size and toggling floating on can start where
    /// the window was.
    fn refresh_tiled_rects(&mut self) {
        let mut placements = Vec::new();
        for output in &self.outputs {
            for workspace in &output.workspaces {
                for Placement { window, rect } in workspace.layout.placements() {
                    placements.push((window, rect));
                }
            }
        }
        for (id, rect) in placements {
            if let Some(window) = self.windows.get_mut(&id) {
                window.set_last_tiled_rect(rect);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Direction;

    fn wm() -> Wm {
        Wm::new(Config::default(), "VEYRON-TEST", Size::new(1280, 800))
    }

    fn open(wm: &mut Wm, count: usize) -> Vec<WindowId> {
        (0..count)
            .map(|index| {
                wm.create_window(NewWindow {
                    app_id: format!("app-{index}"),
                    title: format!("Window {index}"),
                    min_size: None,
                })
            })
            .collect()
    }

    #[test]
    fn new_window_takes_focus_and_gets_a_placement() {
        let mut wm = wm();
        let first = wm.create_window(NewWindow::default());
        let second = wm.create_window(NewWindow::default());

        let snapshot = wm.snapshot();
        assert_eq!(snapshot.placements.len(), 2);
        assert_eq!(snapshot.focused, Some(second));
        assert_eq!(snapshot.columns, 2);
        assert!(snapshot.placement_of(first).is_some());
        assert_eq!(wm.window_count(), 2);
    }

    #[test]
    fn placements_never_exceed_the_output() {
        let mut wm = wm();
        open(&mut wm, 6);
        let snapshot = wm.snapshot();
        for placement in &snapshot.placements {
            // Columns may be scrolled outside the viewport, but their vertical
            // extent and width always match the usable area.
            assert!(placement.rect.y >= snapshot.viewport.y);
            assert!(placement.rect.bottom() <= snapshot.viewport.bottom());
            assert!(placement.rect.w > 0 && placement.rect.h > 0);
            assert!(placement.rect.w <= snapshot.viewport.w);
        }

        let focused = snapshot.focused.expect("a window is focused");
        let rect = snapshot
            .placement_of(focused)
            .expect("focused placement")
            .rect;
        assert_eq!(
            snapshot.output.intersection(rect),
            Some(rect),
            "the focused window is always fully visible"
        );
    }

    #[test]
    fn closing_requests_a_close_but_keeps_the_window() {
        let mut wm = wm();
        let ids = open(&mut wm, 2);
        let effects = wm.dispatch(Action::Close);
        assert_eq!(effects.close_requested, Some(ids[1]));
        assert_eq!(wm.window_count(), 2);

        assert!(wm.remove_window(ids[1]));
        assert_eq!(wm.snapshot().focused, Some(ids[0]));
        assert_eq!(wm.snapshot().placements.len(), 1);
    }

    #[test]
    fn fullscreen_covers_the_viewport_and_restores_afterwards() {
        let mut wm = wm();
        let ids = open(&mut wm, 2);

        let effects = wm.dispatch(Action::ToggleFullscreen);
        assert!(effects.layout_changed);
        let snapshot = wm.snapshot();
        assert_eq!(snapshot.placements.len(), 1);
        assert_eq!(snapshot.placements[0].window, ids[1]);
        assert_eq!(snapshot.placements[0].rect, snapshot.viewport);
        assert!(snapshot.is_fullscreen(ids[1]));

        wm.dispatch(Action::ToggleFullscreen);
        let snapshot = wm.snapshot();
        assert_eq!(snapshot.placements.len(), 2);
        assert!(!snapshot.is_fullscreen(ids[1]));
    }

    #[test]
    fn floating_toggle_round_trips_to_the_previous_geometry() {
        let mut wm = wm();
        let ids = open(&mut wm, 2);
        let tiled = wm
            .snapshot()
            .placement_of(ids[1])
            .map(|p| p.rect)
            .expect("the second window has a placement");

        wm.dispatch(Action::ToggleFloating);
        let snapshot = wm.snapshot();
        assert_eq!(snapshot.placements.len(), 2);
        assert!(snapshot.is_floating(ids[1]));
        assert_eq!(
            snapshot.placement_of(ids[1]).map(|p| p.rect),
            Some(tiled),
            "floating a window keeps it where it was"
        );
        assert_eq!(snapshot.focused, Some(ids[1]));

        wm.dispatch(Action::ToggleFloating);
        let snapshot = wm.snapshot();
        assert!(!snapshot.is_floating(ids[1]));
        assert_eq!(snapshot.focused, Some(ids[1]));
    }

    #[test]
    fn workspaces_are_created_on_demand_and_keep_their_windows() {
        let mut wm = wm();
        let first = wm.create_window(NewWindow::default());

        let effects = wm.dispatch(Action::Workspace { index: 3 });
        assert!(effects.workspace_changed);
        let snapshot = wm.snapshot();
        assert_eq!(snapshot.workspace_index, 2);
        assert_eq!(snapshot.workspace_count, 3);
        assert!(snapshot.placements.is_empty());
        assert_eq!(snapshot.focused, None);

        let second = wm.create_window(NewWindow::default());
        assert_eq!(wm.snapshot().focused, Some(second));

        wm.dispatch(Action::WorkspacePrev);
        wm.dispatch(Action::WorkspacePrev);
        let snapshot = wm.snapshot();
        assert_eq!(snapshot.workspace_index, 0);
        assert_eq!(snapshot.focused, Some(first));
        assert_eq!(snapshot.placements.len(), 1);

        assert!(wm.dispatch(Action::WorkspacePrev).workspace_changed);
        assert_eq!(
            wm.snapshot().workspace_position,
            WorkspacePosition { x: -1, y: 0 }
        );
    }

    #[test]
    fn spatial_navigation_lazily_creates_and_revisits_signed_coordinates() {
        let mut wm = wm();
        let first = wm.create_window(NewWindow::default());

        for direction in [
            Direction::Up,
            Direction::Right,
            Direction::Down,
            Direction::Left,
        ] {
            assert!(
                wm.dispatch(Action::WorkspaceNavigate { direction })
                    .workspace_changed
            );
        }

        let snapshot = wm.snapshot();
        assert_eq!(snapshot.workspace_position, WorkspacePosition::default());
        assert_eq!(snapshot.workspace_count, 4);
        assert_eq!(snapshot.focused, Some(first));
        assert_eq!(snapshot.placements.len(), 1);
    }

    #[test]
    fn moving_a_floating_window_keeps_its_id_mode_and_geometry() {
        let mut wm = wm();
        let id = wm.create_window(NewWindow::default());
        wm.dispatch(Action::ToggleFloating);
        let original = wm
            .snapshot()
            .placement_of(id)
            .map(|placement| placement.rect);

        let effects = wm.dispatch(Action::MoveToWorkspace {
            direction: Direction::Up,
        });
        assert!(effects.workspace_changed);
        let moved = wm.snapshot();
        assert_eq!(moved.workspace_position, WorkspacePosition { x: 0, y: -1 });
        assert_eq!(moved.focused, Some(id));
        assert!(moved.is_floating(id));
        assert_eq!(
            moved.placement_of(id).map(|placement| placement.rect),
            original
        );

        assert!(
            wm.dispatch(Action::WorkspaceNavigate {
                direction: Direction::Down,
            })
            .workspace_changed
        );
        assert!(wm.snapshot().placements.is_empty());
        assert!(
            wm.dispatch(Action::WorkspaceNavigate {
                direction: Direction::Up,
            })
            .workspace_changed
        );
        assert_eq!(wm.snapshot().focused, Some(id));
    }

    #[test]
    fn coordinate_overflow_rejects_navigation_without_mutating_state() {
        let position = WorkspacePosition { x: i64::MAX, y: 0 };
        assert_eq!(position.offset(Direction::Right), None);
        assert_eq!(
            position.offset(Direction::Up),
            Some(WorkspacePosition { x: i64::MAX, y: -1 })
        );
    }

    #[test]
    fn focus_and_move_actions_drive_the_layout() {
        let mut wm = wm();
        let ids = open(&mut wm, 3);

        wm.dispatch(Action::Focus {
            direction: Direction::Left,
        });
        assert_eq!(wm.focused_window(), Some(ids[1]));

        wm.dispatch(Action::Move {
            direction: Direction::Left,
        });
        let snapshot = wm.snapshot();
        assert_eq!(snapshot.columns, 2);
        assert_eq!(snapshot.focused, Some(ids[1]));
        let top = snapshot.placement_of(ids[0]).expect("first window").rect;
        let moved = snapshot.placement_of(ids[1]).expect("moved window").rect;
        assert_eq!(moved.x, top.x, "both windows share a column");
        assert_eq!(moved.w, top.w);
        assert!(
            moved.y > top.y,
            "moved window is stacked under the previous one"
        );
        assert_eq!(moved.bottom(), snapshot.viewport.bottom());
    }

    #[test]
    fn resizing_the_output_reflows_every_window() {
        let mut wm = wm();
        let ids = open(&mut wm, 3);
        wm.set_output_size(Size::new(800, 600));

        let snapshot = wm.snapshot();
        assert_eq!(snapshot.output, Rect::new(0, 0, 800, 600));
        assert_eq!(snapshot.viewport.w, 776);
        for id in ids {
            let rect = snapshot
                .placement_of(id)
                .map(|p| p.rect)
                .expect("every open window has a placement");
            assert!(rect.w <= snapshot.viewport.w);
            assert!(rect.h <= snapshot.viewport.h);
        }
    }

    #[test]
    fn focusing_a_window_from_another_workspace_switches_to_it() {
        let mut wm = wm();
        let ids = open(&mut wm, 2);
        wm.dispatch(Action::Workspace { index: 2 });
        open(&mut wm, 1);

        assert!(wm.focus_window(ids[0]));
        let snapshot = wm.snapshot();
        assert_eq!(snapshot.workspace_index, 0);
        assert_eq!(snapshot.focused, Some(ids[0]));
    }

    #[test]
    fn spawn_and_reload_are_reported_as_effects() {
        let mut wm = wm();
        let effects = wm.dispatch(Action::Spawn {
            command: "foot".into(),
        });
        assert_eq!(effects.spawn, vec!["foot".to_string()]);

        assert!(wm.dispatch(Action::ReloadConfig).reload_config);
        assert!(wm.dispatch(Action::Quit).quit);
    }

    #[test]
    fn removing_a_window_cleans_floating_and_fullscreen_state() {
        let mut wm = wm();
        let ids = open(&mut wm, 1);
        wm.dispatch(Action::ToggleFullscreen);
        assert!(wm.remove_window(ids[0]));

        let snapshot = wm.snapshot();
        assert!(snapshot.placements.is_empty());
        assert_eq!(snapshot.focused, None);
        assert_eq!(wm.workspace_count(), 1);
    }

    #[test]
    fn focus_clicking_a_floating_window_raises_it() {
        let mut wm = wm();
        let ids = open(&mut wm, 2);
        wm.dispatch(Action::Focus {
            direction: Direction::Left,
        });
        wm.dispatch(Action::ToggleFloating);
        assert_eq!(wm.focused_window(), Some(ids[0]));

        wm.focus_window(ids[1]);
        assert_eq!(wm.focused_window(), Some(ids[1]));
    }

    #[test]
    fn zero_sized_output_never_panics_or_produces_invalid_geometry() {
        let mut wm = wm();
        open(&mut wm, 2);
        wm.set_output_size(Size::new(0, 0));
        let snapshot = wm.snapshot();
        assert!(snapshot.output.is_empty());
        assert!(snapshot.viewport.w >= 0);
    }
}
