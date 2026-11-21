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
    action::{Action, Direction, ResizeAxis},
    config::Config,
    geometry::{Rect, Size},
    groups::{GroupId, GroupSet},
    layout::{default_floating_size, viewport_for, Placement, ScrollLayout},
    rules::WindowPolicy,
    session::Session,
    snap::{PipCorner, SnapZone},
    tiling::{arrange, smart_gap, LayoutKind},
    window::{NewWindow, Window, WindowId},
};

/// How many windows smart focus remembers for `focus previous`.
const FOCUS_HISTORY_LIMIT: usize = 32;

/// A workspace is one scrollable strip plus its floating layer.
#[derive(Debug, Clone)]
pub struct Workspace {
    name: String,
    position: WorkspacePosition,
    layout: ScrollLayout,
    /// Layout selected for this workspace. `Auto` is resolved per window count.
    layout_mode: LayoutKind,
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
            layout_mode: config.mode,
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

    /// Layout explicitly selected for this workspace.
    pub fn layout_mode(&self) -> LayoutKind {
        self.layout_mode
    }

    /// Layout actually used for the current window count (`Auto` resolved).
    pub fn effective_layout(&self) -> LayoutKind {
        self.layout_mode.effective(self.layout.window_count())
    }

    pub fn floating_windows(&self) -> &[WindowId] {
        &self.floating
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
    /// Named workspaces, including the default numeric names.
    names: HashMap<String, usize>,
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

    pub fn active_workspace_position(&self) -> WorkspacePosition {
        self.active_workspace().position
    }

    pub fn active_workspace(&self) -> &Workspace {
        &self.workspaces[self.active]
    }

    /// Zero-based index of a workspace by name.
    pub fn workspace_index_by_name(&self, name: &str) -> Option<usize> {
        self.names.get(name).copied()
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacementInfo {
    pub window: WindowId,
    pub rect: Rect,
    pub floating: bool,
    pub fullscreen: bool,
    /// Opacity requested for this window by its rules.
    pub opacity: f32,
    /// Whether the renderer should blur behind this window.
    pub blur: bool,
    /// Whether this window's transitions should be animated.
    pub animate: bool,
    /// Whether the window is docked in a picture-in-picture corner.
    pub pip: bool,
}

/// Everything the compositor needs to draw one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// Full output rectangle, used to clear the frame.
    pub output: Rect,
    /// Name of the output this frame belongs to.
    pub output_name: String,
    /// Index of the output in [`Wm::outputs`].
    pub output_index: usize,
    /// Usable area inside the configured outer margin.
    pub viewport: Rect,
    pub workspace: String,
    pub workspace_index: usize,
    pub workspace_count: usize,
    /// Signed 2D coordinate on the sparse infinite desktop.
    pub workspace_position: WorkspacePosition,
    /// Resolved tiling layout of the active workspace.
    pub layout: LayoutKind,
    /// Effective inner gap in logical pixels (smart gaps applied).
    pub gap: i32,
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

/// Name of the workspace toggled by `workspace special`.
const SPECIAL_WORKSPACE: &str = "special";

/// The Veyron window manager.
#[derive(Debug)]
pub struct Wm {
    config: Config,
    windows: HashMap<WindowId, Window>,
    outputs: Vec<OutputState>,
    active_output: usize,
    next_id: u64,
    /// Windows parked in the scratchpad layer.
    scratchpad: Vec<WindowId>,
    /// Last floating geometry per app id, used by window memory.
    remembered: HashMap<String, Rect>,
    /// Window groups.
    groups: GroupSet,
    /// Recently focused windows, oldest first, used by smart focus.
    focus_history: Vec<WindowId>,
    /// Picture-in-picture corner and the geometry restored when it is turned
    /// off, per window.
    pip: HashMap<WindowId, (PipCorner, Rect)>,
    /// Profile applied most recently, if any.
    active_profile: Option<String>,
    /// Workspace the scratch workspace was opened from.
    special_return: Option<usize>,
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
        let mut wm = Self {
            config,
            windows: HashMap::new(),
            outputs: vec![OutputState {
                name: output_name.into(),
                rect,
                workspaces: vec![workspace],
                workspace_indices,
                names: HashMap::from([("1".to_string(), 0)]),
                active: 0,
            }],
            active_output: 0,
            next_id: 0,
            scratchpad: Vec::new(),
            remembered: HashMap::new(),
            groups: GroupSet::new(),
            focus_history: Vec::new(),
            pip: HashMap::new(),
            active_profile: None,
            special_return: None,
        };
        if let Some(name) = wm.config.default_profile.clone() {
            wm.apply_profile(&name);
        }
        wm.apply_workspace_presets();
        wm
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Apply a new configuration, keeping window and workspace state.
    pub fn set_config(&mut self, config: Config) {
        self.config = config;
        for index in 0..self.outputs.len() {
            let viewport = self.viewport_of(index);
            for workspace in &mut self.outputs[index].workspaces {
                workspace.layout.set_config(self.config.layout.clone());
                workspace.layout.set_viewport(viewport);
            }
        }
        if self
            .active_profile
            .as_deref()
            .is_some_and(|name| self.config.profiles.get(name).is_none())
        {
            self.active_profile = None;
        }
        self.clamp_floating_windows();
        self.refresh_tiled_rects();
        self.apply_workspace_presets();
    }

    /// Name of the profile applied most recently, if any.
    pub fn active_profile(&self) -> Option<&str> {
        self.active_profile.as_deref()
    }

    /// Apply a named profile: its layout patch replaces the matching layout
    /// fields, its motion patch the matching motion fields, and a `mode` in the
    /// patch also selects that layout for every workspace.
    ///
    /// Returns `false` for an unknown name or a patch that would leave the
    /// layout or motion configuration invalid; nothing changes in that case.
    pub fn apply_profile(&mut self, name: &str) -> bool {
        let Some(profile) = self.config.profiles.get(name).cloned() else {
            return false;
        };

        let mut layout = self.config.layout.clone();
        profile.layout.apply(&mut layout);
        if layout.validate().is_err() {
            return false;
        }
        let mut motion = self.config.motion;
        profile.motion.apply(&mut motion);
        if motion.validate().is_err() {
            return false;
        }

        self.config.layout = layout;
        self.config.motion = motion;
        self.active_profile = Some(name.to_string());

        for index in 0..self.outputs.len() {
            let viewport = self.viewport_of(index);
            for workspace in &mut self.outputs[index].workspaces {
                workspace.layout.set_config(self.config.layout.clone());
                workspace.layout.set_viewport(viewport);
                if let Some(mode) = profile.layout.mode {
                    workspace.layout_mode = mode;
                }
            }
        }

        self.clamp_floating_windows();
        self.refresh_tiled_rects();
        true
    }

    pub fn outputs(&self) -> &[OutputState] {
        &self.outputs
    }

    pub fn active_output(&self) -> &OutputState {
        &self.outputs[self.active_output]
    }

    pub fn active_output_index(&self) -> usize {
        self.active_output
    }

    /// Index of an output by name, in compositor order.
    pub fn output_index(&self, name: &str) -> Option<usize> {
        self.outputs.iter().position(|output| output.name == name)
    }

    /// One output by name.
    pub fn output(&self, name: &str) -> Option<&OutputState> {
        let index = self.output_index(name)?;
        self.outputs.get(index)
    }

    /// Usable area of one output, inside the configured outer margin.
    pub fn viewport_of(&self, output_index: usize) -> Rect {
        match self.outputs.get(output_index) {
            Some(output) => viewport_for(output.rect, &self.config.layout),
            None => Rect::ZERO,
        }
    }

    /// Usable area of the active output.
    pub fn viewport(&self) -> Rect {
        self.viewport_of(self.active_output)
    }

    /// Register a new output and give it a first workspace.
    ///
    /// A hot-plugged monitor never steals focus: it is added with workspace
    /// `1` and the active output keeps its workspaces. Returns the index of the
    /// new output, or `None` for a duplicate name or an empty rectangle.
    pub fn add_output(&mut self, name: impl Into<String>, rect: Rect) -> Option<usize> {
        let name = name.into();
        if name.trim().is_empty() || rect.is_empty() || self.output_index(&name).is_some() {
            return None;
        }
        let viewport = viewport_for(rect, &self.config.layout);
        let index = self.outputs.len();
        self.outputs.push(OutputState {
            name,
            rect,
            workspaces: vec![Workspace::new(
                "1".to_string(),
                WorkspacePosition::default(),
                &self.config.layout,
                viewport,
            )],
            workspace_indices: HashMap::from([(WorkspacePosition::default(), 0)]),
            names: HashMap::from([("1".to_string(), 0)]),
            active: 0,
        });
        Some(index)
    }

    /// Unplug an output.
    ///
    /// Its windows are migrated to the active workspace of the output that
    /// stays active, floating windows are clamped into that output's viewport
    /// and picture-in-picture windows are re-docked to the same corner. The
    /// last remaining output is never removed. Returns whether the output went
    /// away.
    pub fn remove_output(&mut self, name: &str) -> bool {
        let Some(index) = self.output_index(name) else {
            return false;
        };
        if self.outputs.len() <= 1 {
            return false;
        }

        let mut tiled = Vec::new();
        let mut floating = Vec::new();
        for workspace in &self.outputs[index].workspaces {
            tiled.extend(workspace.layout.order());
            floating.extend(workspace.floating.iter().copied());
        }

        let removed_active = self.active_output == index;
        self.outputs.remove(index);
        if removed_active {
            self.active_output = index.min(self.outputs.len() - 1);
        } else if self.active_output > index {
            self.active_output -= 1;
        }

        let output_index = self.active_output;
        let workspace_index = self.outputs[output_index].active;
        let viewport = self.viewport();
        for id in tiled {
            self.outputs[output_index].workspaces[workspace_index]
                .layout
                .insert_window(id);
        }
        for id in floating {
            if let Some(window) = self.windows.get(&id) {
                if let Some((corner, _)) = self.pip.get(&id).copied() {
                    let rect = corner.rect(viewport, crate::snap::DEFAULT_PIP_RATIO, 0);
                    if let Some(window) = self.windows.get_mut(&id) {
                        window.set_floating_rect(rect);
                    }
                } else {
                    let rect = clamp_into(window.floating_rect(), viewport);
                    if let Some(window) = self.windows.get_mut(&id) {
                        window.set_floating_rect(rect);
                    }
                }
            }
            let workspace = &mut self.outputs[output_index].workspaces[workspace_index];
            workspace.floating.retain(|window| *window != id);
            workspace.floating.push(id);
            workspace.focused_floating = Some(id);
        }

        self.refresh_tiled_rects();
        self.note_focus();
        true
    }

    /// Make an output the one that receives focus and new windows.
    pub fn focus_output(&mut self, index: usize) -> bool {
        if index >= self.outputs.len() || index == self.active_output {
            return false;
        }
        self.active_output = index;
        self.note_focus();
        true
    }

    /// The output that lies in `direction` from the active one.
    ///
    /// A candidate only counts when it sits beyond the active output's edge in
    /// that direction, so two monitors side by side are not also "above" each
    /// other. The closest one wins, with the cross-axis distance as a
    /// tie-break.
    pub fn adjacent_output(&self, direction: Direction) -> Option<usize> {
        let origin = self.outputs[self.active_output].rect;
        let mut best: Option<((i64, i64), usize)> = None;
        for (index, output) in self.outputs.iter().enumerate() {
            if index == self.active_output {
                continue;
            }
            let rect = output.rect;
            let (along, across) = match direction {
                Direction::Left if rect.right() <= origin.x => (
                    i64::from(origin.x) - i64::from(rect.right()),
                    axis_distance(rect, origin, true),
                ),
                Direction::Right if rect.x >= origin.right() => (
                    i64::from(rect.x) - i64::from(origin.right()),
                    axis_distance(rect, origin, true),
                ),
                Direction::Up if rect.bottom() <= origin.y => (
                    i64::from(origin.y) - i64::from(rect.bottom()),
                    axis_distance(rect, origin, false),
                ),
                Direction::Down if rect.y >= origin.bottom() => (
                    i64::from(rect.y) - i64::from(origin.bottom()),
                    axis_distance(rect, origin, false),
                ),
                _ => continue,
            };
            let score = (along, across);
            if best.is_none_or(|(current, _)| score < current) {
                best = Some((score, index));
            }
        }
        best.map(|(_, index)| index)
    }

    /// Move focus to the output in `direction`, if there is one.
    pub fn focus_output_direction(&mut self, direction: Direction) -> bool {
        match self.adjacent_output(direction) {
            Some(index) => self.focus_output(index),
            None => false,
        }
    }

    /// Move the focused window to another output's active workspace, keeping its
    /// floating/fullscreen state and keeping a floating window inside the new
    /// viewport. Focus follows the window.
    pub fn move_focused_to_output(&mut self, index: usize) -> bool {
        if index >= self.outputs.len() {
            return false;
        }
        let Some(id) = self.focused_window() else {
            return false;
        };
        let Some((source_output, source_workspace, floating)) = self.locate(id) else {
            return false;
        };
        if source_output == index {
            return false;
        }

        let fullscreen =
            self.outputs[source_output].workspaces[source_workspace].fullscreen == Some(id);
        let destination_workspace = self.outputs[index].active;
        if fullscreen
            && self.outputs[index].workspaces[destination_workspace]
                .fullscreen
                .is_some()
        {
            return false;
        }

        let source = &mut self.outputs[source_output].workspaces[source_workspace];
        source.layout.remove_window(id);
        source.floating.retain(|window| *window != id);
        if source.focused_floating == Some(id) {
            source.focused_floating = None;
        }
        if source.fullscreen == Some(id) {
            source.fullscreen = None;
        }

        let viewport = self.viewport_of(index);
        if floating {
            let rect = match self.pip.get(&id).copied() {
                Some((corner, _)) => corner.rect(viewport, crate::snap::DEFAULT_PIP_RATIO, 0),
                None => self
                    .windows
                    .get(&id)
                    .map(|window| clamp_into(window.floating_rect(), viewport))
                    .unwrap_or_else(|| viewport.centered(default_floating_size(viewport))),
            };
            if let Some(window) = self.windows.get_mut(&id) {
                window.set_floating_rect(rect);
            }
        }

        let destination = &mut self.outputs[index].workspaces[destination_workspace];
        if floating {
            destination.floating.push(id);
            destination.focused_floating = Some(id);
        } else {
            destination.focused_floating = None;
            destination.layout.insert_window(id);
        }
        if fullscreen {
            destination.fullscreen = Some(id);
        }

        self.active_output = index;
        self.refresh_tiled_rects();
        self.note_focus();
        true
    }

    /// Move the whole active workspace, including its windows, to another
    /// output and follow it there.
    ///
    /// Refuses when the target already has a workspace with that name or when
    /// the source output would be left without any workspace.
    pub fn move_active_workspace_to_output(&mut self, index: usize) -> bool {
        if index >= self.outputs.len() {
            return false;
        }
        let source_output = self.active_output;
        if source_output == index || self.outputs[source_output].workspaces.len() <= 1 {
            return false;
        }
        let source_workspace = self.outputs[source_output].active;
        let name = self.outputs[source_output].workspaces[source_workspace]
            .name
            .clone();
        if self.outputs[index].names.contains_key(&name) {
            return false;
        }

        let position = self.next_linear_position(index);
        let mut workspace = self.outputs[source_output]
            .workspaces
            .remove(source_workspace);
        workspace.position = position;
        let viewport = self.viewport_of(index);
        workspace.layout.set_viewport(viewport);

        let remaining = self.outputs[source_output].workspaces.len();
        if remaining > 0 {
            let active = source_workspace.min(remaining - 1);
            self.outputs[source_output].active = active;
        }
        self.rebuild_workspace_indices(source_output);

        let floating = workspace.floating.clone();
        self.outputs[index].workspaces.push(workspace);
        self.rebuild_workspace_indices(index);
        let destination = self.outputs[index].workspaces.len() - 1;
        self.outputs[index].active = destination;
        self.active_output = index;

        for id in floating {
            if self.windows.contains_key(&id) {
                self.clamp_window_into_viewport(id, index);
            }
        }

        self.refresh_tiled_rects();
        self.note_focus();
        true
    }

    /// Move the active workspace to the output in `direction`, if there is one.
    pub fn move_active_workspace_in_direction(&mut self, direction: Direction) -> bool {
        match self.adjacent_output(direction) {
            Some(index) => self.move_active_workspace_to_output(index),
            None => false,
        }
    }

    /// Move the focused window to the output in `direction`, if there is one.
    pub fn move_focused_in_direction(&mut self, direction: Direction) -> bool {
        match self.adjacent_output(direction) {
            Some(index) => self.move_focused_to_output(index),
            None => false,
        }
    }

    fn clamp_window_into_viewport(&mut self, id: WindowId, output_index: usize) {
        let viewport = self.viewport_of(output_index);
        let rect = match self.pip.get(&id).copied() {
            Some((corner, _)) => corner.rect(viewport, crate::snap::DEFAULT_PIP_RATIO, 0),
            None => match self.windows.get(&id) {
                Some(window) => clamp_into(window.floating_rect(), viewport),
                None => return,
            },
        };
        if let Some(window) = self.windows.get_mut(&id) {
            window.set_floating_rect(rect);
        }
    }

    fn rebuild_workspace_indices(&mut self, output_index: usize) {
        let output = &mut self.outputs[output_index];
        output.workspace_indices.clear();
        output.names.clear();
        for (index, workspace) in output.workspaces.iter().enumerate() {
            output.workspace_indices.insert(workspace.position, index);
            output.names.insert(workspace.name.clone(), index);
        }
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

    /// Windows parked in the scratchpad layer (visible or hidden).
    pub fn scratchpad_windows(&self) -> &[WindowId] {
        &self.scratchpad
    }

    pub fn workspace_count(&self) -> usize {
        self.active_output().workspaces.len()
    }

    pub fn focused_window(&self) -> Option<WindowId> {
        self.active_output().active_workspace().focused_window()
    }

    /// Create a managed window, applying its rules.
    ///
    /// Rules can make a window floating, give it opacity/blur, park it in the
    /// scratchpad, route it to a named workspace or switch that workspace to a
    /// layout. Without rules the window opens tiled on the active workspace,
    /// which is the original behaviour.
    pub fn create_window(&mut self, request: NewWindow) -> WindowId {
        self.create_window_in(request, None)
    }

    /// Create a window on an explicit workspace; `None` keeps the rule-driven
    /// routing (a rule-provided workspace, otherwise the active one). Session
    /// restore uses this and rules still apply.
    pub fn create_window_in(&mut self, request: NewWindow, workspace: Option<&str>) -> WindowId {
        self.next_id += 1;
        let id = WindowId(self.next_id);
        // A toplevel exists before its client announces anything, so the
        // compositor creates windows without an app id or title. Such a window
        // is not matched here; `set_window_identity` applies the rules once the
        // first commit delivers the client's identity.
        let named = !request.app_id.is_empty() || !request.title.is_empty();
        let policy = if named {
            self.config.rules.resolve(&request.app_id, &request.title)
        } else {
            WindowPolicy::default()
        };

        let mut window = Window::new(id, request);
        window.set_rules_applied(named);
        window.set_opacity(policy.opacity);
        window.set_blur(policy.blur);
        window.set_animate(policy.animate);
        let viewport = self.viewport();
        let floating_size = window
            .min_size()
            .map(|size| size.clamp_to(viewport.size()))
            .unwrap_or_else(|| default_floating_size(viewport));
        let remembered = self
            .remembered
            .get(window.app_id())
            .copied()
            .filter(|rect| !rect.is_empty())
            .map(|rect| {
                let size = rect.size().clamp_to(viewport.size());
                Rect::new(rect.x, rect.y, size.w, size.h)
            });
        window.set_floating_rect(remembered.unwrap_or_else(|| viewport.centered(floating_size)));
        if policy.floating {
            window.set_floating(true);
        }
        if policy.scratchpad {
            window.set_floating(true);
        }
        self.windows.insert(id, window);

        let output_index = self.active_output;
        let target = match workspace.or(policy.workspace.as_deref()) {
            Some(name) => self.find_or_create_named_workspace(output_index, name),
            None => self.outputs[output_index].active,
        };
        let is_active = target == self.outputs[output_index].active;
        if let Some(kind) = policy.layout {
            self.outputs[output_index].workspaces[target].layout_mode = kind;
        }

        if policy.scratchpad {
            self.scratchpad.push(id);
            self.refresh_tiled_rects();
            self.note_focus();
            return id;
        }

        let workspace = &mut self.outputs[output_index].workspaces[target];
        if policy.floating {
            workspace.floating.push(id);
            if is_active {
                workspace.focused_floating = Some(id);
            }
        } else {
            if is_active {
                workspace.focused_floating = None;
            }
            workspace.layout.insert_window(id);
        }

        self.refresh_tiled_rects();
        self.note_focus();
        id
    }

    /// Forget a window. Windows disappear when their surface is destroyed,
    /// including after a close request, so this is the single removal path.
    pub fn remove_window(&mut self, id: WindowId) -> bool {
        if self.windows.remove(&id).is_none() {
            return false;
        }
        self.scratchpad.retain(|window| *window != id);
        self.pip.remove(&id);
        self.focus_history.retain(|window| *window != id);
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
        self.prune_groups();
        true
    }

    /// Update the metadata a client announced for one of its windows.
    ///
    /// A toplevel exists before its client sends `set_app_id`/`set_title`, so
    /// the compositor learns the identity on the first commit and reports it
    /// here. The first non-empty identity applies the configured rules exactly
    /// once — opacity, blur, floating, scratchpad, workspace routing and
    /// layout — while later title changes only refresh the metadata.
    pub fn set_window_identity(&mut self, id: WindowId, app_id: &str, title: &str) -> Effects {
        let mut effects = Effects::default();
        let Some(window) = self.windows.get_mut(&id) else {
            return effects;
        };
        window.set_app_id(app_id.to_string());
        window.set_title(title.to_string());
        if (app_id.is_empty() && title.is_empty()) || window.rules_applied() {
            return effects;
        }
        window.set_rules_applied(true);

        let policy = self.config.rules.resolve(app_id, title);
        window.set_opacity(policy.opacity);
        window.set_blur(policy.blur);
        window.set_animate(policy.animate);

        if policy.scratchpad {
            self.park_window(id);
            effects.layout_changed = true;
            effects.focus_changed = true;
            return effects;
        }

        if policy.floating {
            let fallback = self
                .windows
                .get(&id)
                .map(Window::floating_rect)
                .unwrap_or(Rect::ZERO);
            let rect = self
                .remembered
                .get(app_id)
                .copied()
                .filter(|rect| !rect.is_empty())
                .unwrap_or(fallback);
            effects.layout_changed |= self.make_floating(id, rect);
        }

        if let Some(name) = policy.workspace.as_deref() {
            effects.layout_changed |= self.route_window(id, name);
        }

        if let Some(kind) = policy.layout {
            if let Some((output_index, workspace_index, _)) = self.locate(id) {
                self.outputs[output_index].workspaces[workspace_index].layout_mode = kind;
                effects.layout_changed = true;
            }
        }

        if effects.layout_changed {
            self.note_focus();
        }
        effects
    }

    /// Move a window to the named workspace, creating it on demand. Returns
    /// whether the window changed workspace.
    fn route_window(&mut self, id: WindowId, name: &str) -> bool {
        let Some((output_index, current, floating)) = self.locate(id) else {
            return false;
        };
        let target = self.find_or_create_named_workspace(output_index, name);
        if target == current {
            return false;
        }
        self.detach_window(id);
        let workspace = &mut self.outputs[output_index].workspaces[target];
        if floating {
            workspace.floating.push(id);
        } else {
            workspace.layout.insert_window(id);
        }
        self.refresh_tiled_rects();
        true
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

        let changed = previous != self.focused_window();
        self.note_focus();
        changed
    }

    /// Apply the next (`delta > 0`) or previous configured profile, wrapping
    /// around. Without an active profile this starts at the first (or last)
    /// one, so the default chord works before any profile is applied by hand.
    fn cycle_profile(&mut self, delta: isize) -> bool {
        let profiles = self.config.profiles.as_slice();
        if profiles.is_empty() {
            return false;
        }
        let index = match self.active_profile.as_deref() {
            Some(active) => {
                let current = profiles
                    .iter()
                    .position(|profile| profile.name == active)
                    .unwrap_or(0);
                (current as isize + delta).rem_euclid(profiles.len() as isize) as usize
            }
            None if delta >= 0 => 0,
            None => profiles.len() - 1,
        };
        let name = profiles[index].name.clone();
        self.apply_profile(&name)
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
                effects.focus_changed = focus_in_layout(workspace, direction);
                effects.layout_changed = effects.focus_changed;
            }
            Action::Move { direction } => {
                let workspace = self.active_workspace_mut();
                workspace.focused_floating = None;
                effects.layout_changed = move_in_layout(workspace, direction);
            }
            Action::MoveFloating { direction, delta } => {
                effects.layout_changed = self.move_focused_floating(direction, delta);
            }
            Action::ColumnWidth { delta } => {
                let workspace = self.active_workspace_mut();
                // Column ratios only exist in the scrolling strip.
                if workspace.effective_layout() == LayoutKind::Scroll {
                    effects.layout_changed = workspace.layout.resize_focused(delta);
                }
            }
            Action::Layout { kind } => {
                effects.layout_changed = self.set_layout(kind);
            }
            Action::LayoutNext => {
                let kind = self.active_output().active_workspace().layout_mode.next();
                effects.layout_changed = self.set_layout(kind);
            }
            Action::ToggleScratchpad => {
                effects.layout_changed = self.toggle_scratchpad();
                effects.focus_changed = effects.layout_changed;
            }
            Action::Resize { axis, delta } => {
                effects.layout_changed = self.resize_focused_floating(axis, delta);
            }
            Action::CenterColumn => {
                let workspace = self.active_workspace_mut();
                if workspace.effective_layout() == LayoutKind::Scroll {
                    effects.layout_changed = workspace.layout.center_focus();
                }
            }
            Action::ToggleFullscreen => effects.layout_changed = self.toggle_fullscreen(),
            Action::ToggleFloating => effects.layout_changed = self.toggle_floating(),
            Action::Snap { zone } => {
                effects.layout_changed = self.snap_focused(zone);
                effects.focus_changed = effects.layout_changed;
            }
            Action::Pip { corner } => {
                effects.layout_changed = self.toggle_pip(corner);
                effects.focus_changed = effects.layout_changed;
            }
            Action::FocusPrevious => {
                effects.focus_changed = self.focus_previous();
                effects.layout_changed = effects.focus_changed;
            }
            Action::GroupToggle => {
                effects.layout_changed = self.toggle_group();
            }
            Action::GroupCycle { delta } => {
                effects.focus_changed = self.cycle_group(delta);
                effects.layout_changed = effects.focus_changed;
            }
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
            Action::WorkspaceSpecial => {
                effects.workspace_changed = self.toggle_special_workspace();
                effects.layout_changed = effects.workspace_changed;
                effects.focus_changed = effects.workspace_changed;
            }
            Action::OutputFocus { direction } => {
                effects.focus_changed = self.focus_output_direction(direction);
                effects.workspace_changed = effects.focus_changed;
                effects.layout_changed = effects.focus_changed;
            }
            Action::MoveToOutput { direction } => {
                effects.workspace_changed = self.move_focused_in_direction(direction);
                effects.layout_changed = effects.workspace_changed;
                effects.focus_changed = effects.workspace_changed;
            }
            Action::WorkspaceToOutput { direction } => {
                effects.workspace_changed = self.move_active_workspace_in_direction(direction);
                effects.layout_changed = effects.workspace_changed;
                effects.focus_changed = effects.workspace_changed;
            }
            Action::Profile { name } => {
                effects.layout_changed = self.apply_profile(&name);
            }
            Action::ProfileNext => effects.layout_changed = self.cycle_profile(1),
            Action::ProfilePrev => effects.layout_changed = self.cycle_profile(-1),
        }

        if effects.layout_changed {
            self.refresh_tiled_rects();
        }
        self.note_focus();
        effects
    }

    /// Update the active output's size (window resize in the nested backend, or
    /// a mode change on real hardware).
    pub fn set_output_size(&mut self, size: Size) {
        self.set_output_rect(self.active_output, Rect::from_size(size));
    }

    /// Update one output's rectangle, reflowing only that output. Returns
    /// whether the rectangle actually changed.
    pub fn set_output_rect(&mut self, output_index: usize, rect: Rect) -> bool {
        if output_index >= self.outputs.len() || self.outputs[output_index].rect == rect {
            return false;
        }
        let viewport = viewport_for(rect, &self.config.layout);
        self.outputs[output_index].rect = rect;
        for workspace in &mut self.outputs[output_index].workspaces {
            workspace.layout.set_viewport(viewport);
        }
        self.clamp_floating_windows();
        self.refresh_tiled_rects();
        true
    }

    /// Build the frame description of the active output.
    pub fn snapshot(&self) -> Snapshot {
        self.build_snapshot(self.active_output)
    }

    /// Frame description of one output, `None` when the index is out of range.
    pub fn output_snapshot(&self, output_index: usize) -> Option<Snapshot> {
        if output_index >= self.outputs.len() {
            return None;
        }
        Some(self.build_snapshot(output_index))
    }

    /// One frame description per output, in compositor order.
    pub fn snapshots(&self) -> Vec<Snapshot> {
        (0..self.outputs.len())
            .map(|index| self.build_snapshot(index))
            .collect()
    }

    fn build_snapshot(&self, output_index: usize) -> Snapshot {
        let output = &self.outputs[output_index];
        let workspace = output.active_workspace();
        let viewport = viewport_for(output.rect, &self.config.layout);
        let layout = workspace.effective_layout();
        let gap = smart_gap(&self.config.layout, workspace.layout.window_count());

        let mut placements = Vec::with_capacity(workspace.layout.window_count());
        let visual = |window: WindowId| -> (f32, bool, bool) {
            self.windows
                .get(&window)
                .map(|window| (window.opacity(), window.blur(), window.animate()))
                .unwrap_or((1.0, false, true))
        };

        match workspace.fullscreen {
            Some(id) if self.windows.contains_key(&id) => {
                let (opacity, blur, animate) = visual(id);
                placements.push(PlacementInfo {
                    window: id,
                    rect: viewport,
                    floating: self
                        .windows
                        .get(&id)
                        .map(Window::is_floating)
                        .unwrap_or(false),
                    fullscreen: true,
                    opacity,
                    blur,
                    animate,
                    pip: self.pip.contains_key(&id),
                });
            }
            _ => {
                let tiled = if layout == LayoutKind::Scroll {
                    workspace.layout.placements_with_gap(gap)
                } else {
                    arrange(
                        layout,
                        &workspace.layout.order(),
                        viewport,
                        gap,
                        self.config.layout.master_ratio,
                    )
                };
                for Placement { window, rect } in tiled {
                    let (opacity, blur, animate) = visual(window);
                    placements.push(PlacementInfo {
                        window,
                        rect,
                        floating: false,
                        fullscreen: false,
                        opacity,
                        blur,
                        animate,
                        pip: false,
                    });
                }
                for id in &workspace.floating {
                    if let Some(window) = self.windows.get(id) {
                        placements.push(PlacementInfo {
                            window: *id,
                            rect: window.floating_rect(),
                            floating: true,
                            fullscreen: false,
                            opacity: window.opacity(),
                            blur: window.blur(),
                            animate: window.animate(),
                            pip: self.pip.contains_key(id),
                        });
                    }
                }
            }
        }

        Snapshot {
            output: output.rect,
            output_name: output.name.clone(),
            output_index,
            viewport,
            workspace: workspace.name.clone(),
            workspace_index: output.active,
            workspace_count: output.workspaces.len(),
            workspace_position: workspace.position,
            layout,
            gap,
            columns: workspace.layout.column_count(),
            scroll: if layout == LayoutKind::Scroll {
                workspace.layout.scroll()
            } else {
                0.0
            },
            focused: workspace.focused_window(),
            placements,
        }
    }

    /// Most recently focused windows, oldest first.
    pub fn focus_history(&self) -> &[WindowId] {
        &self.focus_history
    }

    /// Every window group of the session.
    pub fn groups(&self) -> &GroupSet {
        &self.groups
    }

    /// The group a window belongs to, if any.
    pub fn group_of(&self, id: WindowId) -> Option<GroupId> {
        self.groups.group_of(id)
    }

    /// Whether the window is docked in a picture-in-picture corner.
    pub fn is_pip(&self, id: WindowId) -> bool {
        self.pip.contains_key(&id)
    }

    /// Capture the session for Veyron Recall.
    pub fn capture_session(&self) -> Session {
        Session::capture(self)
    }

    /// Restore a captured session: recreate every window in its workspace and
    /// reinstate floating geometry. Returns how many windows were restored.
    pub fn restore_session(&mut self, session: &Session) -> usize {
        for name in &session.workspaces {
            let output_index = self.active_output;
            self.find_or_create_named_workspace(output_index, name);
        }

        let mut restored = 0;
        for record in &session.windows {
            let id = self.create_window_in(
                NewWindow {
                    app_id: record.app_id.clone(),
                    title: record.title.clone(),
                    min_size: None,
                },
                Some(&record.workspace),
            );
            if record.scratchpad {
                self.park_window(id);
            } else if record.floating {
                let viewport = self.viewport();
                let rect = record
                    .rect
                    .unwrap_or_else(|| viewport.centered(default_floating_size(viewport)));
                self.make_floating(id, rect);
            }
            restored += 1;
        }
        self.note_focus();
        restored
    }

    /// Park a window in the scratchpad layer.
    fn park_window(&mut self, id: WindowId) {
        self.detach_window(id);
        if !self.scratchpad.contains(&id) {
            self.scratchpad.push(id);
        }
        if let Some(window) = self.windows.get_mut(&id) {
            window.set_floating(true);
        }
        self.refresh_tiled_rects();
    }

    /// Put a window in the floating layer with an explicit geometry, clamped to
    /// the viewport. Returns whether anything changed.
    fn make_floating(&mut self, id: WindowId, rect: Rect) -> bool {
        let Some((output_index, workspace_index, floating)) = self.locate(id) else {
            return false;
        };

        if !floating {
            let workspace = &mut self.outputs[output_index].workspaces[workspace_index];
            workspace.layout.remove_window(id);
        }

        let viewport = self.viewport_of(output_index);
        let target = clamp_into(rect, viewport);

        let workspace = &mut self.outputs[output_index].workspaces[workspace_index];
        workspace.floating.retain(|window| *window != id);
        workspace.floating.push(id);
        workspace.focused_floating = Some(id);
        if workspace.fullscreen == Some(id) {
            workspace.fullscreen = None;
        }

        let Some(window) = self.windows.get_mut(&id) else {
            return false;
        };
        let changed = !window.is_floating() || window.floating_rect() != target;
        window.set_floating(true);
        window.set_floating_rect(target);
        changed
    }

    /// Record the focused window in the smart-focus history.
    fn note_focus(&mut self) {
        let Some(id) = self.focused_window() else {
            return;
        };
        self.focus_history.retain(|window| *window != id);
        self.focus_history.push(id);
        if self.focus_history.len() > FOCUS_HISTORY_LIMIT {
            self.focus_history.remove(0);
        }
    }

    /// The window focused before the current one, if it still exists.
    fn previous_window(&self) -> Option<WindowId> {
        let current = self.focused_window();
        self.focus_history
            .iter()
            .rev()
            .copied()
            .find(|id| Some(*id) != current && self.windows.contains_key(id))
    }

    /// Jump to the previously focused window (smart focus).
    fn focus_previous(&mut self) -> bool {
        match self.previous_window() {
            Some(id) => self.focus_window(id),
            None => false,
        }
    }

    /// Snap the focused window to a zone, floating it first if it is tiled.
    fn snap_focused(&mut self, zone: SnapZone) -> bool {
        let Some(id) = self.focused_window() else {
            return false;
        };
        if !self.windows.contains_key(&id) {
            return false;
        }
        let viewport = self.viewport();
        let gap = smart_gap(&self.config.layout, 1).max(0);
        let rect = zone.rect(viewport, gap);
        self.pip.remove(&id);
        self.make_floating(id, rect)
    }

    /// Toggle picture-in-picture for the focused window.
    fn toggle_pip(&mut self, corner: PipCorner) -> bool {
        let Some(id) = self.focused_window() else {
            return false;
        };

        if let Some((_, previous)) = self.pip.remove(&id) {
            let viewport = self.viewport();
            let rect = if previous.is_empty() {
                viewport.centered(default_floating_size(viewport))
            } else {
                previous
            };
            return self.make_floating(id, rect);
        }

        let viewport = self.viewport();
        let previous = self
            .windows
            .get(&id)
            .map(Window::floating_rect)
            .filter(|rect| !rect.is_empty())
            .unwrap_or_else(|| viewport.centered(default_floating_size(viewport)));
        let rect = corner.rect(viewport, crate::snap::DEFAULT_PIP_RATIO, 0);
        let changed = self.make_floating(id, rect);
        self.pip.insert(id, (corner, previous));
        changed
    }

    /// Group the focused window with the previous one, or ungroup it.
    fn toggle_group(&mut self) -> bool {
        let Some(id) = self.focused_window() else {
            return false;
        };
        let with = self.previous_window();
        let before = self.groups.group_of(id);
        self.groups.toggle(id, with);
        let after = self.groups.group_of(id);
        if before == after {
            return false;
        }
        self.regroup_active_workspace();
        true
    }

    /// Move focus to the next/previous member of the focused window's group.
    fn cycle_group(&mut self, delta: i32) -> bool {
        let Some(id) = self.focused_window() else {
            return false;
        };
        let Some(target) = self.groups.cycle(id, delta as isize) else {
            return false;
        };
        self.focus_window(target)
    }

    /// Keep the members of a group adjacent in the strip.
    fn regroup_active_workspace(&mut self) {
        let output_index = self.active_output;
        let workspace_index = self.outputs[output_index].active;
        let current = self.outputs[output_index].workspaces[workspace_index]
            .layout
            .order();
        if current.is_empty() {
            return;
        }

        let mut desired: Vec<WindowId> = Vec::with_capacity(current.len());
        let mut placed: Vec<GroupId> = Vec::new();
        for window in &current {
            match self.groups.group_of(*window) {
                Some(group) => {
                    if placed.contains(&group) {
                        continue;
                    }
                    placed.push(group);
                    for member in self.groups.members_of(*window) {
                        if current.contains(member) && !desired.contains(member) {
                            desired.push(*member);
                        }
                    }
                }
                None => desired.push(*window),
            }
        }
        self.outputs[output_index].workspaces[workspace_index]
            .layout
            .reorder(&desired);
    }

    /// Forget windows that no longer exist and drop empty groups.
    fn prune_groups(&mut self) {
        let alive: std::collections::HashSet<WindowId> = self.windows.keys().copied().collect();
        self.groups.prune(&alive);
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

    /// Show the scratch workspace, or return to the one it was opened from.
    ///
    /// The workspace is created on demand and named `special`, so rules can
    /// route applications into it like any other named workspace.
    fn toggle_special_workspace(&mut self) -> bool {
        let output_index = self.active_output;
        let active = self.outputs[output_index].active;
        if self.outputs[output_index]
            .names
            .get(SPECIAL_WORKSPACE)
            .copied()
            == Some(active)
        {
            let target = self
                .special_return
                .filter(|index| {
                    *index != active && *index < self.outputs[output_index].workspaces.len()
                })
                .unwrap_or(0);
            self.special_return = None;
            return self.switch_workspace(target);
        }

        let special = self.find_or_create_named_workspace(output_index, SPECIAL_WORKSPACE);
        if special == active {
            return false;
        }
        self.special_return = Some(active);
        self.switch_workspace(special)
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
        // Numeric names are the default labels; skip any name a rule already
        // created so two workspaces can never share a label.
        let mut number = self.outputs[output_index].workspaces.len() + 1;
        loop {
            let name = number.to_string();
            if !self.outputs[output_index].names.contains_key(&name) {
                return self.append_workspace_with_name(output_index, position, name);
            }
            number += 1;
        }
    }

    fn append_workspace_with_name(
        &mut self,
        output_index: usize,
        position: WorkspacePosition,
        name: String,
    ) -> usize {
        let output = &mut self.outputs[output_index];
        let index = output.workspaces.len();
        let viewport = viewport_for(output.rect, &self.config.layout);
        output.workspaces.push(Workspace::new(
            name.clone(),
            position,
            &self.config.layout,
            viewport,
        ));
        output.workspace_indices.insert(position, index);
        output.names.insert(name, index);
        index
    }

    /// Find a named workspace on an output, creating it at the next free
    /// horizontal coordinate when it does not exist yet.
    fn find_or_create_named_workspace(&mut self, output_index: usize, name: &str) -> usize {
        if let Some(index) = self.outputs[output_index].names.get(name).copied() {
            return index;
        }
        let position = self.next_linear_position(output_index);
        self.append_workspace_with_name(output_index, position, name.to_string())
    }

    /// Create the workspaces the configuration pins, on the primary output.
    ///
    /// A preset is a configuration concept, not a monitor one, so presets land
    /// on the primary output; `move-output` and `workspace-output` move content
    /// afterwards. Existing names are left alone, which makes this safe to run
    /// again after a configuration reload. A pinned position already taken by
    /// another workspace falls back to the next free horizontal slot.
    fn apply_workspace_presets(&mut self) {
        let presets = self.config.workspaces.clone();
        for preset in presets {
            if self.outputs[0].names.contains_key(&preset.name) {
                continue;
            }
            let mut position = match preset.position() {
                Some((x, y)) => WorkspacePosition { x, y },
                None => self.next_linear_position(0),
            };
            if self.outputs[0].workspace_indices.contains_key(&position) {
                position = self.next_linear_position(0);
            }
            let index = self.append_workspace_with_name(0, position, preset.name.clone());
            if let Some(kind) = preset.layout {
                self.outputs[0].workspaces[index].layout_mode = kind;
            }
        }
    }

    /// Select the tiling layout of the active workspace.
    fn set_layout(&mut self, kind: LayoutKind) -> bool {
        let workspace = self.active_workspace_mut();
        if workspace.layout_mode == kind {
            return false;
        }
        workspace.layout_mode = kind;
        true
    }

    /// Remove a window from every workspace without forgetting it.
    fn detach_window(&mut self, id: WindowId) {
        for output in &mut self.outputs {
            for workspace in &mut output.workspaces {
                workspace.floating.retain(|window| *window != id);
                workspace.layout.remove_window(id);
                if workspace.focused_floating == Some(id) {
                    workspace.focused_floating = None;
                }
                if workspace.fullscreen == Some(id) {
                    workspace.fullscreen = None;
                }
            }
        }
    }

    /// Remember where an application's floating window was placed.
    fn remember_geometry(&mut self, id: WindowId) {
        if !self.config.behavior.remember_geometry {
            return;
        }
        let Some(window) = self.windows.get(&id) else {
            return;
        };
        if !window.is_floating() || window.app_id().is_empty() {
            return;
        }
        let app_id = window.app_id().to_string();
        let rect = window.floating_rect();
        self.remembered.insert(app_id, rect);
    }

    /// Show every scratchpad window on the active workspace, or hide them all
    /// again when they are already placed somewhere.
    fn toggle_scratchpad(&mut self) -> bool {
        // With nothing parked yet, the action parks the focused window. This
        // makes the scratchpad usable with a single keybinding and no rules.
        if self.scratchpad.is_empty() {
            let Some(id) = self.focused_window() else {
                return false;
            };
            self.detach_window(id);
            self.scratchpad.push(id);
            if let Some(window) = self.windows.get_mut(&id) {
                window.set_floating(true);
            }
            self.refresh_tiled_rects();
            return true;
        }

        let stored: Vec<WindowId> = self.scratchpad.clone();
        let visible = stored.iter().any(|id| self.locate(*id).is_some());
        let output_index = self.active_output;
        let workspace_index = self.outputs[output_index].active;

        if visible {
            for id in &stored {
                self.detach_window(*id);
            }
        } else {
            let viewport = self.viewport();
            let mut offset = 0;
            for id in &stored {
                let Some(window) = self.windows.get(id) else {
                    continue;
                };
                let mut rect = if window.floating_rect().is_empty() {
                    viewport.centered(default_floating_size(viewport))
                } else {
                    window.floating_rect()
                };
                // Cascade scratchpad windows so several can be seen at once.
                rect.x = rect
                    .x
                    .saturating_add(24 * offset)
                    .clamp(viewport.x, (viewport.right() - rect.w).max(viewport.x));
                rect.y = rect
                    .y
                    .saturating_add(24 * offset)
                    .clamp(viewport.y, (viewport.bottom() - rect.h).max(viewport.y));
                if let Some(window) = self.windows.get_mut(id) {
                    window.set_floating(true);
                    window.set_floating_rect(rect);
                }
                offset += 1;
            }

            let last = stored
                .iter()
                .rev()
                .copied()
                .find(|id| self.windows.contains_key(id));
            let workspace = &mut self.outputs[output_index].workspaces[workspace_index];
            for id in &stored {
                if self.windows.contains_key(id) {
                    workspace.floating.retain(|window| window != id);
                    workspace.floating.push(*id);
                }
            }
            workspace.focused_floating = last;
        }

        self.refresh_tiled_rects();
        true
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
            self.remember_geometry(id);
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

    fn resize_focused_floating(&mut self, axis: ResizeAxis, delta: i32) -> bool {
        let Some(id) = self.focused_window() else {
            return false;
        };
        let Some(window) = self.windows.get(&id) else {
            return false;
        };
        if !window.is_floating() {
            return false;
        }

        let mut rect = window.floating_rect();
        let requested_min = window.min_size().unwrap_or(Size::new(64, 48));
        let viewport = self.viewport();
        let (old_size, new_size, min_size, origin, extent) = match axis {
            ResizeAxis::Width => (
                rect.w,
                rect.w.saturating_add(delta),
                requested_min.w.max(64).min(viewport.w),
                rect.x - viewport.x,
                viewport.w,
            ),
            ResizeAxis::Height => (
                rect.h,
                rect.h.saturating_add(delta),
                requested_min.h.max(48).min(viewport.h),
                rect.y - viewport.y,
                viewport.h,
            ),
        };
        let new_size = new_size.clamp(min_size, extent);
        if old_size == new_size {
            return false;
        }

        let new_origin = origin
            .saturating_sub((new_size - old_size) / 2)
            .clamp(0, extent - new_size);
        match axis {
            ResizeAxis::Width => {
                rect.x = viewport.x + new_origin;
                rect.w = new_size;
            }
            ResizeAxis::Height => {
                rect.y = viewport.y + new_origin;
                rect.h = new_size;
            }
        }
        let changed = if let Some(window) = self.windows.get_mut(&id) {
            window.set_floating_rect(rect);
            true
        } else {
            false
        };
        if changed {
            self.remember_geometry(id);
        }
        changed
    }

    fn move_focused_floating(&mut self, direction: crate::action::Direction, delta: i32) -> bool {
        if delta <= 0 {
            return false;
        }
        let Some(id) = self.focused_window() else {
            return false;
        };
        let Some(window) = self.windows.get(&id) else {
            return false;
        };
        if !window.is_floating() {
            return false;
        }

        let mut rect = window.floating_rect();
        let viewport = self.viewport();
        let (dx, dy) = match direction {
            crate::action::Direction::Left => (-delta, 0),
            crate::action::Direction::Right => (delta, 0),
            crate::action::Direction::Up => (0, -delta),
            crate::action::Direction::Down => (0, delta),
        };
        let max_x = (viewport.right() - rect.w).max(viewport.x);
        let max_y = (viewport.bottom() - rect.h).max(viewport.y);
        rect.x = rect.x.saturating_add(dx).clamp(viewport.x, max_x);
        rect.y = rect.y.saturating_add(dy).clamp(viewport.y, max_y);
        let changed = if let Some(window) = self.windows.get_mut(&id) {
            let changed = window.floating_rect() != rect;
            window.set_floating_rect(rect);
            changed
        } else {
            false
        };
        if changed {
            self.remember_geometry(id);
        }
        changed
    }

    /// Keep floating windows on screen after an output resize or margin change.
    /// Each window is clamped to the viewport of the output it belongs to.
    fn clamp_floating_windows(&mut self) {
        for index in 0..self.outputs.len() {
            let viewport = self.viewport_of(index);
            for workspace in &self.outputs[index].workspaces {
                for id in &workspace.floating {
                    if let Some(window) = self.windows.get_mut(id) {
                        let rect = window.floating_rect();
                        window.set_floating_rect(clamp_into(rect, viewport));
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

/// Distance between two rectangles along one axis, measured between their
/// nearest edges: the vertical gap for side-by-side monitors, the horizontal gap
/// for stacked ones.
fn axis_distance(rect: Rect, origin: Rect, horizontal: bool) -> i64 {
    let (start, end, other_start, other_end) = if horizontal {
        (rect.y, rect.bottom(), origin.y, origin.bottom())
    } else {
        (rect.x, rect.right(), origin.x, origin.right())
    };
    let gap = (other_start - end).max(start - other_end).max(0);
    i64::from(gap)
}

/// Fit a rectangle inside a viewport, shrinking it first when it is too big.
fn clamp_into(rect: Rect, viewport: Rect) -> Rect {
    let size = rect.size().clamp_to(viewport.size());
    let x = rect
        .x
        .clamp(viewport.x, (viewport.right() - size.w).max(viewport.x));
    let y = rect
        .y
        .clamp(viewport.y, (viewport.bottom() - size.h).max(viewport.y));
    Rect::new(x, y, size.w, size.h)
}

/// Neighbour offset in flat strip order for layouts without 2D neighbours.
fn order_delta(direction: Direction) -> isize {
    match direction {
        Direction::Left | Direction::Up => -1,
        Direction::Right | Direction::Down => 1,
    }
}

/// Move focus in the direction the *resolved* layout understands.
fn focus_in_layout(workspace: &mut Workspace, direction: Direction) -> bool {
    if workspace.effective_layout() == LayoutKind::Scroll {
        workspace.layout.focus_direction(direction)
    } else {
        workspace
            .layout
            .focus_order_neighbor(order_delta(direction))
    }
}

/// Move the focused window in the direction the resolved layout understands.
fn move_in_layout(workspace: &mut Workspace, direction: Direction) -> bool {
    if workspace.effective_layout() == LayoutKind::Scroll {
        workspace.layout.move_window(direction)
    } else {
        workspace.layout.move_order_neighbor(order_delta(direction))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Direction;
    use crate::rules::WindowRule;

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
    fn floating_resize_respects_client_minimum_and_viewport_bounds() {
        let mut wm = wm();
        let id = wm.create_window(NewWindow {
            min_size: Some(Size::new(200, 100)),
            ..NewWindow::default()
        });
        wm.dispatch(Action::ToggleFloating);

        assert!(
            wm.dispatch(Action::Resize {
                axis: ResizeAxis::Width,
                delta: 10_000,
            })
            .layout_changed
        );
        let expanded = wm
            .snapshot()
            .placement_of(id)
            .expect("floating placement")
            .rect;
        let viewport = wm.snapshot().viewport;
        assert_eq!(expanded.w, viewport.w);
        assert!(expanded.x >= viewport.x);
        assert!(expanded.right() <= viewport.right());

        assert!(
            wm.dispatch(Action::Resize {
                axis: ResizeAxis::Height,
                delta: -64,
            })
            .layout_changed
        );
        assert!(
            wm.dispatch(Action::Resize {
                axis: ResizeAxis::Height,
                delta: 64,
            })
            .layout_changed
        );
        assert!(
            wm.dispatch(Action::Resize {
                axis: ResizeAxis::Height,
                delta: -10_000,
            })
            .layout_changed
        );
        let resized = wm
            .snapshot()
            .placement_of(id)
            .expect("floating placement")
            .rect;
        assert_eq!(resized.h, 100);
        assert!(resized.y >= viewport.y);
        assert!(resized.bottom() <= viewport.bottom());
    }

    #[test]
    fn resize_action_does_not_change_a_tiled_window() {
        let mut wm = wm();
        let id = wm.create_window(NewWindow::default());
        let original = wm
            .snapshot()
            .placement_of(id)
            .expect("tiled placement")
            .rect;
        assert!(
            !wm.dispatch(Action::Resize {
                axis: ResizeAxis::Width,
                delta: 32,
            })
            .layout_changed
        );
        assert_eq!(
            wm.snapshot()
                .placement_of(id)
                .map(|placement| placement.rect),
            Some(original)
        );
    }

    #[test]
    fn floating_move_keeps_geometry_inside_the_viewport() {
        let mut wm = wm();
        let id = wm.create_window(NewWindow::default());
        wm.dispatch(Action::ToggleFloating);
        let initial = wm
            .snapshot()
            .placement_of(id)
            .expect("floating placement")
            .rect;

        assert!(
            wm.dispatch(Action::MoveFloating {
                direction: Direction::Right,
                delta: 32,
            })
            .layout_changed
        );
        let moved = wm
            .snapshot()
            .placement_of(id)
            .expect("floating placement")
            .rect;
        assert_eq!(moved.x, initial.x + 32);
        assert_eq!(moved.size(), initial.size());

        assert!(
            wm.dispatch(Action::MoveFloating {
                direction: Direction::Left,
                delta: i32::MAX,
            })
            .layout_changed
        );
        let clamped = wm
            .snapshot()
            .placement_of(id)
            .expect("floating placement")
            .rect;
        let viewport = wm.snapshot().viewport;
        assert_eq!(clamped.x, viewport.x);
        assert!(clamped.y >= viewport.y);
        assert!(clamped.right() <= viewport.right());
        assert!(clamped.bottom() <= viewport.bottom());
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

    fn wm_with_rules(rules: Vec<WindowRule>) -> Wm {
        let config = Config {
            rules: crate::rules::RuleSet::new(rules).expect("valid rules"),
            ..Config::default()
        };
        Wm::new(config, "VEYRON-TEST", Size::new(1280, 800))
    }

    fn rule(app_id: &str) -> WindowRule {
        WindowRule {
            app_id: Some(crate::rules::Glob::new(app_id).expect("valid glob")),
            ..WindowRule::default()
        }
    }

    #[test]
    fn switching_to_master_stack_retiles_and_reorders_focus() {
        let mut wm = wm();
        let ids = open(&mut wm, 3);

        assert!(
            wm.dispatch(Action::Layout {
                kind: LayoutKind::MasterStack
            })
            .layout_changed
        );
        let snapshot = wm.snapshot();
        assert_eq!(snapshot.layout, LayoutKind::MasterStack);
        assert_eq!(snapshot.placements.len(), 3);
        assert_eq!(
            snapshot.placement_of(ids[0]).expect("master").rect,
            Rect::new(12, 12, 749, 776)
        );
        assert_eq!(snapshot.focused, Some(ids[2]));

        // In a flat layout left/right move through the strip order.
        assert!(
            wm.dispatch(Action::Focus {
                direction: Direction::Left
            })
            .focus_changed
        );
        assert_eq!(wm.focused_window(), Some(ids[1]));
        assert!(
            wm.dispatch(Action::Move {
                direction: Direction::Left
            })
            .layout_changed
        );
        let snapshot = wm.snapshot();
        assert_eq!(wm.focused_window(), Some(ids[1]));
        assert_eq!(
            snapshot.placement_of(ids[1]).expect("master").rect.w,
            749,
            "the moved window became the master"
        );
    }

    #[test]
    fn layout_next_cycles_and_auto_resolves_from_window_count() {
        let mut wm = wm();
        open(&mut wm, 4);

        // Default is scroll; next is master-stack, then dwindle, then auto.
        assert!(wm.dispatch(Action::LayoutNext).layout_changed);
        assert_eq!(wm.snapshot().layout, LayoutKind::MasterStack);
        assert!(wm.dispatch(Action::LayoutNext).layout_changed);
        assert_eq!(wm.snapshot().layout, LayoutKind::Dwindle);
        assert!(wm.dispatch(Action::LayoutNext).layout_changed);
        let snapshot = wm.snapshot();
        assert_eq!(snapshot.layout, LayoutKind::Dwindle, "auto picks dwindle");
        assert_eq!(
            wm.active_output().active_workspace().layout_mode(),
            LayoutKind::Auto
        );
    }

    #[test]
    fn smart_gaps_remove_the_gap_for_a_lone_window() {
        let config = Config {
            layout: crate::layout::LayoutConfig {
                smart_gaps: true,
                mode: LayoutKind::MasterStack,
                ..crate::layout::LayoutConfig::default()
            },
            ..Config::default()
        };
        let mut wm = Wm::new(config, "VEYRON-TEST", Size::new(1000, 800));
        let id = wm.create_window(NewWindow::default());

        let snapshot = wm.snapshot();
        assert_eq!(snapshot.gap, 0);
        assert_eq!(
            snapshot.placement_of(id).expect("placement").rect,
            snapshot.viewport,
            "a lone tiled window fills the usable area"
        );
    }

    #[test]
    fn rules_make_matching_windows_float_with_visual_properties() {
        let mut wm = wm_with_rules(vec![WindowRule {
            app_id: Some(crate::rules::Glob::new("mpv").expect("glob")),
            floating: Some(true),
            opacity: Some(0.85),
            blur: Some(true),
            ..WindowRule::default()
        }]);

        let floating = wm.create_window(NewWindow {
            app_id: "mpv".into(),
            ..NewWindow::default()
        });
        let tiled = wm.create_window(NewWindow {
            app_id: "alacritty".into(),
            ..NewWindow::default()
        });

        let snapshot = wm.snapshot();
        assert!(snapshot.is_floating(floating));
        assert!(!snapshot.is_floating(tiled));
        let placement = snapshot.placement_of(floating).expect("placement");
        assert_eq!(placement.opacity, 0.85);
        assert!(placement.blur);
        assert_eq!(
            snapshot.placement_of(tiled).expect("placement").opacity,
            1.0
        );
    }

    #[test]
    fn rules_route_windows_to_a_named_workspace_with_its_layout() {
        let mut wm = wm_with_rules(vec![WindowRule {
            workspace: Some("chat".into()),
            layout: Some(LayoutKind::Dwindle),
            ..rule("signal")
        }]);

        let id = wm.create_window(NewWindow {
            app_id: "signal".into(),
            ..NewWindow::default()
        });
        let snapshot = wm.snapshot();
        assert_eq!(snapshot.workspace, "1");
        assert!(
            snapshot.placements.is_empty(),
            "routing does not steal focus"
        );

        let index = wm.outputs()[0]
            .workspace_index_by_name("chat")
            .expect("named workspace exists");
        assert!(
            wm.dispatch(Action::Workspace { index: index + 1 })
                .workspace_changed
        );
        let snapshot = wm.snapshot();
        assert_eq!(snapshot.workspace, "chat");
        assert_eq!(snapshot.layout, LayoutKind::Dwindle);
        assert!(snapshot.placement_of(id).is_some());
    }

    #[test]
    fn scratchpad_toggles_windows_between_hidden_and_floating() {
        let mut wm = wm();
        let id = wm.create_window(NewWindow::default());

        assert!(wm.dispatch(Action::ToggleScratchpad).layout_changed);
        assert_eq!(wm.scratchpad_windows(), &[id]);
        let snapshot = wm.snapshot();
        assert!(snapshot.placements.is_empty(), "scratchpad is hidden");
        assert_eq!(snapshot.focused, None);

        assert!(wm.dispatch(Action::ToggleScratchpad).layout_changed);
        let snapshot = wm.snapshot();
        assert_eq!(snapshot.placements.len(), 1);
        assert!(snapshot.is_floating(id));
        assert_eq!(snapshot.focused, Some(id));

        wm.remove_window(id);
        assert!(wm.scratchpad_windows().is_empty());
    }

    #[test]
    fn scratchpad_rules_park_windows_out_of_the_workspace() {
        let mut wm = wm_with_rules(vec![WindowRule {
            scratchpad: Some(true),
            ..rule("pavucontrol")
        }]);
        let id = wm.create_window(NewWindow {
            app_id: "pavucontrol".into(),
            ..NewWindow::default()
        });

        assert!(wm.snapshot().placements.is_empty());
        assert_eq!(wm.scratchpad_windows(), &[id]);
        assert!(wm.dispatch(Action::ToggleScratchpad).layout_changed);
        assert!(wm.snapshot().placement_of(id).is_some());
    }

    #[test]
    fn late_window_identity_applies_rules_once() {
        let mut wm = wm_with_rules(vec![WindowRule {
            floating: Some(true),
            opacity: Some(0.6),
            ..rule("firefox")
        }]);

        // The compositor creates the window before its client announces itself.
        let id = wm.create_window(NewWindow::default());
        let window = wm.window(id).expect("window");
        assert_eq!(window.app_id(), "");
        assert!(!window.is_floating());
        assert_eq!(window.opacity(), 1.0);

        let effects = wm.set_window_identity(id, "firefox", "Start Page");
        assert!(
            effects.layout_changed,
            "the floating rule reflows the window"
        );
        let window = wm.window(id).expect("window");
        assert_eq!(window.app_id(), "firefox");
        assert_eq!(window.title(), "Start Page");
        assert!(window.is_floating());
        assert_eq!(window.opacity(), 0.6);

        let again = wm.set_window_identity(id, "firefox", "Later Title");
        assert_eq!(again, Effects::default(), "rules are applied only once");
        assert_eq!(wm.window(id).expect("window").title(), "Later Title");
    }

    #[test]
    fn late_window_identity_routes_to_the_rule_workspace() {
        let mut wm = wm_with_rules(vec![WindowRule {
            workspace: Some("chat".into()),
            layout: Some(LayoutKind::Dwindle),
            ..rule("signal")
        }]);

        let id = wm.create_window(NewWindow::default());
        wm.set_window_identity(id, "signal", "Signal");

        let index = wm.outputs()[0]
            .workspace_index_by_name("chat")
            .expect("routing creates the workspace");
        assert!(wm.outputs()[0].workspaces()[index]
            .layout()
            .order()
            .contains(&id));
        assert_eq!(
            wm.outputs()[0].workspaces()[index].layout_mode(),
            LayoutKind::Dwindle
        );
        assert_eq!(wm.snapshot().workspace, "1", "routing does not steal focus");
        assert!(wm.snapshot().placements.is_empty());
    }

    #[test]
    fn window_memory_restores_floating_geometry_per_app() {
        let mut wm = wm_with_rules(vec![WindowRule {
            floating: Some(true),
            ..rule("mpv")
        }]);
        let first = wm.create_window(NewWindow {
            app_id: "mpv".into(),
            ..NewWindow::default()
        });
        assert!(
            wm.dispatch(Action::MoveFloating {
                direction: Direction::Right,
                delta: 40,
            })
            .layout_changed
        );
        let moved = wm.snapshot().placement_of(first).expect("placement").rect;

        assert!(wm.remove_window(first));
        let second = wm.create_window(NewWindow {
            app_id: "mpv".into(),
            ..NewWindow::default()
        });
        assert_eq!(
            wm.snapshot().placement_of(second).expect("placement").rect,
            moved,
            "a new window of the same app restores the remembered geometry"
        );
    }

    #[test]
    fn column_resize_is_ignored_outside_the_scrolling_layout() {
        let mut wm = wm();
        wm.create_window(NewWindow::default());
        wm.dispatch(Action::Layout {
            kind: LayoutKind::MasterStack,
        });
        assert!(
            !wm.dispatch(Action::ColumnWidth { delta: 20 })
                .layout_changed
        );
    }

    #[test]
    fn configured_workspaces_exist_at_startup_with_their_position_and_layout() {
        let config = Config::from_toml_str(
            "[[workspaces]]\nname = \"code\"\nx = 2\ny = 0\nlayout = \"dwindle\"\n\n\
             [[workspaces]]\nname = \"chat\"\n\n\
             [[workspaces]]\nname = \"zero\"\nx = 0\ny = 0\n",
        )
        .expect("valid config");
        let wm = Wm::new(config, "VEYRON-TEST", Size::new(1280, 800));
        let output = wm.active_output();

        assert_eq!(
            output.workspaces().len(),
            4,
            "three presets plus workspace 1"
        );
        let code = output.workspace_index_by_name("code").expect("code preset");
        assert_eq!(
            output.workspaces()[code].position(),
            WorkspacePosition { x: 2, y: 0 }
        );
        assert_eq!(output.workspaces()[code].layout_mode(), LayoutKind::Dwindle);

        let chat = output.workspace_index_by_name("chat").expect("chat preset");
        let zero = output.workspace_index_by_name("zero").expect("zero preset");
        assert_ne!(
            output.workspaces()[chat].position(),
            output.workspaces()[zero].position(),
            "a taken position is never reused"
        );
        assert_ne!(
            output.workspaces()[zero].position(),
            WorkspacePosition { x: 0, y: 0 },
            "workspace 1 already sits at the origin"
        );
        assert_eq!(wm.active_output().active_workspace().name(), "1");
    }

    #[test]
    fn the_special_workspace_toggles_back_to_where_the_user_came_from() {
        let mut wm = wm();
        wm.create_window(NewWindow::default());
        assert!(
            wm.dispatch(Action::Workspace { index: 2 })
                .workspace_changed
        );

        let effects = wm.dispatch(Action::WorkspaceSpecial);
        assert!(effects.workspace_changed);
        assert_eq!(
            wm.active_output().active_workspace().name(),
            "special",
            "the scratch workspace is created on demand"
        );

        let effects = wm.dispatch(Action::WorkspaceSpecial);
        assert!(effects.workspace_changed);
        assert_eq!(
            wm.active_output().active_workspace().name(),
            "2",
            "toggling again returns to the previous workspace"
        );
    }
}
