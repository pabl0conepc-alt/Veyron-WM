//! Actions are the single vocabulary shared by keybindings and IPC.
//!
//! Every user-visible operation is an [`Action`]. Keybindings map a chord to an
//! action, `veyronctl` sends the textual form over IPC, and the state machine
//! dispatches the same enum. There is exactly one parser, so a binding and an
//! IPC command can never drift apart.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{
    snap::{PipCorner, SnapZone},
    tiling::LayoutKind,
};

/// A spatial direction used by focus and move actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// Dimension of a floating window changed by a resize action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResizeAxis {
    Width,
    Height,
}

impl fmt::Display for ResizeAxis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ResizeAxis::Width => "width",
            ResizeAxis::Height => "height",
        })
    }
}

impl Direction {
    pub const ALL: [Direction; 4] = [
        Direction::Left,
        Direction::Right,
        Direction::Up,
        Direction::Down,
    ];

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "left" | "l" | "h" => Some(Direction::Left),
            "right" | "r" => Some(Direction::Right),
            "up" | "u" | "k" => Some(Direction::Up),
            "down" | "d" | "j" => Some(Direction::Down),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Left => "left",
            Direction::Right => "right",
            Direction::Up => "up",
            Direction::Down => "down",
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when an action string cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid action: {0}")]
pub struct ActionParseError(pub String);

impl ActionParseError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// Everything the window manager can be asked to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum Action {
    /// Ask the focused window to close (the client decides).
    Close,
    /// Move focus to a neighbouring column (`left`/`right`) or row (`up`/`down`).
    Focus { direction: Direction },
    /// Move the focused window into the neighbouring column or swap it with the
    /// neighbouring row.
    Move { direction: Direction },
    /// Move the focused floating window by logical pixels.
    MoveFloating { direction: Direction, delta: i32 },
    /// Resize the focused column by a percentage of the viewport.
    ColumnWidth { delta: i32 },
    /// Resize the focused floating window by logical pixels.
    Resize { axis: ResizeAxis, delta: i32 },
    /// Switch to a 1-based workspace index, creating it when needed.
    Workspace { index: usize },
    /// Navigate to an adjacent spatial workspace, creating it on demand.
    WorkspaceNavigate { direction: Direction },
    /// Move the focused window to an adjacent workspace and activate it.
    MoveToWorkspace { direction: Direction },
    /// Switch to the next workspace, creating it when at the end.
    WorkspaceNext,
    /// Switch to the previous workspace.
    WorkspacePrev,
    /// Move focus to the output (monitor) in that direction.
    OutputFocus { direction: Direction },
    /// Move the focused window to the output in that direction.
    MoveToOutput { direction: Direction },
    /// Move the active workspace, windows included, to the output in that
    /// direction and follow it there.
    WorkspaceToOutput { direction: Direction },
    /// Apply a named layout/motion profile.
    Profile { name: String },
    /// Select the tiling layout of the active workspace.
    Layout { kind: LayoutKind },
    /// Cycle the active workspace to the next tiling layout.
    LayoutNext,
    /// Show the scratchpad windows, or hide them again.
    ToggleScratchpad,
    /// Toggle fullscreen for the focused window.
    ToggleFullscreen,
    /// Toggle floating for the focused window.
    ToggleFloating,
    /// Scroll the focused column into the center of the viewport.
    CenterColumn,
    /// Snap the focused window to a screen zone, floating it when needed.
    Snap { zone: SnapZone },
    /// Toggle picture-in-picture for the focused window.
    Pip { corner: PipCorner },
    /// Focus the previously focused window (smart focus).
    FocusPrevious,
    /// Add the focused window to the previous window's group, or leave it.
    GroupToggle,
    /// Cycle focus inside the focused window's group.
    GroupCycle { delta: i32 },
    /// Run a shell command.
    Spawn { command: String },
    /// Reload the configuration file.
    ReloadConfig,
    /// Exit the compositor.
    Quit,
}

impl Action {
    /// Parse the canonical text form used by IPC and the configuration file.
    pub fn parse(input: &str) -> Result<Action, ActionParseError> {
        let text = input.trim();
        if text.is_empty() {
            return Err(ActionParseError::new("empty action"));
        }

        let (head, rest) = match text.split_once(char::is_whitespace) {
            Some((head, rest)) => (head, rest.trim()),
            None => (text, ""),
        };

        match head.to_ascii_lowercase().as_str() {
            "close" | "close-window" => without_argument(rest, Action::Close),
            "focus" => {
                let value = rest.to_ascii_lowercase();
                if value == "previous" || value == "last" {
                    return Ok(Action::FocusPrevious);
                }
                Ok(Action::Focus {
                    direction: parse_direction(rest)?,
                })
            }
            "move" => Ok(Action::Move {
                direction: parse_direction(rest)?,
            }),
            "move-floating" => {
                let (direction, amount) =
                    rest.split_once(char::is_whitespace).ok_or_else(|| {
                        ActionParseError::new("move-floating needs a direction and pixel distance")
                    })?;
                let direction = parse_direction(direction)?;
                let delta = amount.trim().parse::<i32>().map_err(|_| {
                    ActionParseError::new(format!(
                        "move-floating needs a positive integer pixel distance, got '{amount}'"
                    ))
                })?;
                if delta <= 0 {
                    return Err(ActionParseError::new(
                        "move-floating pixel distance must be positive",
                    ));
                }
                Ok(Action::MoveFloating { direction, delta })
            }
            "column-width" | "width" => {
                let delta = rest.parse::<i32>().map_err(|_| {
                    ActionParseError::new(format!(
                        "column-width needs an integer percentage, got '{rest}'"
                    ))
                })?;
                Ok(Action::ColumnWidth { delta })
            }
            "resize" => {
                let (axis, amount) = rest.split_once(char::is_whitespace).ok_or_else(|| {
                    ActionParseError::new("resize needs a dimension and pixel delta")
                })?;
                let axis = match axis.to_ascii_lowercase().as_str() {
                    "width" => ResizeAxis::Width,
                    "height" => ResizeAxis::Height,
                    _ => {
                        return Err(ActionParseError::new(
                            "resize dimension must be width or height",
                        ))
                    }
                };
                let delta = amount.trim().parse::<i32>().map_err(|_| {
                    ActionParseError::new(format!(
                        "resize needs an integer pixel delta, got '{amount}'"
                    ))
                })?;
                if delta == 0 {
                    return Err(ActionParseError::new("resize pixel delta cannot be zero"));
                }
                Ok(Action::Resize { axis, delta })
            }
            "workspace" => {
                let value = rest.to_ascii_lowercase();
                match value.as_str() {
                    "next" => Ok(Action::WorkspaceNext),
                    "prev" | "previous" => Ok(Action::WorkspacePrev),
                    "left" | "right" | "up" | "down" => {
                        if let Some(direction) = Direction::parse(&value) {
                            Ok(Action::WorkspaceNavigate { direction })
                        } else {
                            Err(ActionParseError::new(format!(
                                "unknown workspace direction '{rest}'"
                            )))
                        }
                    }
                    _ => {
                        let index = rest.parse::<usize>().map_err(|_| {
                            ActionParseError::new(format!(
                                "workspace needs 'next', 'prev' or a 1-based index, got '{rest}'"
                            ))
                        })?;
                        if index == 0 {
                            return Err(ActionParseError::new("workspace indexes start at 1"));
                        }
                        Ok(Action::Workspace { index })
                    }
                }
            }
            "move-workspace" | "send-to-workspace" => Ok(Action::MoveToWorkspace {
                direction: parse_direction(rest)?,
            }),
            "output" | "focus-output" => Ok(Action::OutputFocus {
                direction: parse_direction(rest)?,
            }),
            "move-output" | "move-to-output" => Ok(Action::MoveToOutput {
                direction: parse_direction(rest)?,
            }),
            "workspace-output" | "move-workspace-to-output" => Ok(Action::WorkspaceToOutput {
                direction: parse_direction(rest)?,
            }),
            "profile" | "apply-profile" => {
                if rest.is_empty() {
                    return Err(ActionParseError::new("profile needs a name"));
                }
                if rest.split_whitespace().count() != 1 {
                    return Err(ActionParseError::new(
                        "profile takes exactly one name without whitespace",
                    ));
                }
                Ok(Action::Profile {
                    name: rest.to_string(),
                })
            }
            "layout" | "set-layout" => {
                if rest.eq_ignore_ascii_case("next") {
                    return Ok(Action::LayoutNext);
                }
                Ok(Action::Layout {
                    kind: rest.parse::<LayoutKind>()?,
                })
            }
            "layout-next" | "next-layout" => without_argument(rest, Action::LayoutNext),
            "scratchpad" | "toggle-scratchpad" => without_argument(rest, Action::ToggleScratchpad),
            "toggle-fullscreen" | "fullscreen" => without_argument(rest, Action::ToggleFullscreen),
            "toggle-floating" | "floating" => without_argument(rest, Action::ToggleFloating),
            "center" | "center-column" => without_argument(rest, Action::CenterColumn),
            "snap" => Ok(Action::Snap {
                zone: rest.parse::<SnapZone>()?,
            }),
            "pip" | "toggle-pip" | "picture-in-picture" => Ok(Action::Pip {
                corner: if rest.is_empty() {
                    PipCorner::default()
                } else {
                    rest.parse::<PipCorner>()?
                },
            }),
            "focus-previous" | "focus-last" | "last-window" => {
                without_argument(rest, Action::FocusPrevious)
            }
            "group" | "toggle-group" => match rest.to_ascii_lowercase().as_str() {
                "" => Ok(Action::GroupToggle),
                "next" => Ok(Action::GroupCycle { delta: 1 }),
                "prev" | "previous" => Ok(Action::GroupCycle { delta: -1 }),
                other => Err(ActionParseError::new(format!(
                    "group takes no argument, 'next' or 'prev', got '{other}'"
                ))),
            },
            "group-next" => without_argument(rest, Action::GroupCycle { delta: 1 }),
            "group-prev" | "group-previous" => {
                without_argument(rest, Action::GroupCycle { delta: -1 })
            }
            "spawn" | "exec" => {
                if rest.is_empty() {
                    return Err(ActionParseError::new("spawn needs a command"));
                }
                Ok(Action::Spawn {
                    command: rest.to_string(),
                })
            }
            "reload" | "reload-config" => without_argument(rest, Action::ReloadConfig),
            "quit" | "exit" => without_argument(rest, Action::Quit),
            other => Err(ActionParseError::new(format!("unknown action '{other}'"))),
        }
    }
}

fn without_argument(rest: &str, action: Action) -> Result<Action, ActionParseError> {
    if rest.is_empty() {
        Ok(action)
    } else {
        Err(ActionParseError::new(format!(
            "action takes no argument, got '{rest}'"
        )))
    }
}

fn parse_direction(value: &str) -> Result<Direction, ActionParseError> {
    if value.is_empty() {
        return Err(ActionParseError::new("direction is missing"));
    }
    Direction::parse(value)
        .ok_or_else(|| ActionParseError::new(format!("unknown direction '{value}'")))
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Close => f.write_str("close"),
            Action::Focus { direction } => write!(f, "focus {direction}"),
            Action::Move { direction } => write!(f, "move {direction}"),
            Action::MoveFloating { direction, delta } => {
                write!(f, "move-floating {direction} {delta}")
            }
            Action::ColumnWidth { delta } => write!(f, "column-width {delta}"),
            Action::Resize { axis, delta } => write!(f, "resize {axis} {delta}"),
            Action::Workspace { index } => write!(f, "workspace {index}"),
            Action::WorkspaceNavigate { direction } => write!(f, "workspace {direction}"),
            Action::MoveToWorkspace { direction } => write!(f, "move-workspace {direction}"),
            Action::WorkspaceNext => f.write_str("workspace next"),
            Action::WorkspacePrev => f.write_str("workspace prev"),
            Action::OutputFocus { direction } => write!(f, "output {direction}"),
            Action::MoveToOutput { direction } => write!(f, "move-output {direction}"),
            Action::WorkspaceToOutput { direction } => {
                write!(f, "workspace-output {direction}")
            }
            Action::Profile { name } => write!(f, "profile {name}"),
            Action::Layout { kind } => write!(f, "layout {kind}"),
            Action::LayoutNext => f.write_str("layout next"),
            Action::ToggleScratchpad => f.write_str("scratchpad"),
            Action::ToggleFullscreen => f.write_str("toggle-fullscreen"),
            Action::ToggleFloating => f.write_str("toggle-floating"),
            Action::CenterColumn => f.write_str("center-column"),
            Action::Snap { zone } => write!(f, "snap {zone}"),
            Action::Pip { corner } => write!(f, "pip {corner}"),
            Action::FocusPrevious => f.write_str("focus previous"),
            Action::GroupToggle => f.write_str("group"),
            Action::GroupCycle { delta } if *delta > 0 => f.write_str("group next"),
            Action::GroupCycle { .. } => f.write_str("group prev"),
            Action::Spawn { command } => write!(f, "spawn {command}"),
            Action::ReloadConfig => f.write_str("reload"),
            Action::Quit => f.write_str("quit"),
        }
    }
}

impl FromStr for Action {
    type Err = ActionParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Action::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_round_trips_through_text() {
        let actions = vec![
            Action::Close,
            Action::Focus {
                direction: Direction::Left,
            },
            Action::Move {
                direction: Direction::Down,
            },
            Action::MoveFloating {
                direction: Direction::Right,
                delta: 32,
            },
            Action::ColumnWidth { delta: -10 },
            Action::ColumnWidth { delta: 25 },
            Action::Resize {
                axis: ResizeAxis::Height,
                delta: 32,
            },
            Action::Workspace { index: 3 },
            Action::WorkspaceNavigate {
                direction: Direction::Up,
            },
            Action::MoveToWorkspace {
                direction: Direction::Right,
            },
            Action::WorkspaceNext,
            Action::WorkspacePrev,
            Action::OutputFocus {
                direction: Direction::Right,
            },
            Action::MoveToOutput {
                direction: Direction::Left,
            },
            Action::WorkspaceToOutput {
                direction: Direction::Up,
            },
            Action::Profile {
                name: "coding".into(),
            },
            Action::Layout {
                kind: LayoutKind::Dwindle,
            },
            Action::Layout {
                kind: LayoutKind::Auto,
            },
            Action::LayoutNext,
            Action::ToggleScratchpad,
            Action::ToggleFullscreen,
            Action::ToggleFloating,
            Action::CenterColumn,
            Action::Snap {
                zone: SnapZone::LeftThird,
            },
            Action::Snap {
                zone: SnapZone::Maximize,
            },
            Action::Pip {
                corner: PipCorner::BottomRight,
            },
            Action::Pip {
                corner: PipCorner::TopLeft,
            },
            Action::FocusPrevious,
            Action::GroupToggle,
            Action::GroupCycle { delta: 1 },
            Action::GroupCycle { delta: -1 },
            Action::Spawn {
                command: "alacritty --class veyron".into(),
            },
            Action::ReloadConfig,
            Action::Quit,
        ];

        for action in actions {
            let text = action.to_string();
            assert_eq!(Action::parse(&text).as_ref(), Ok(&action), "text: {text}");
        }
    }

    #[test]
    fn aliases_and_directions_are_accepted() {
        assert_eq!(
            Action::parse("focus h"),
            Ok(Action::Focus {
                direction: Direction::Left
            })
        );
        assert_eq!(Action::parse("FOCUS   LEFT"), Action::parse("focus left"));
        assert_eq!(
            Action::parse("exec foot"),
            Ok(Action::Spawn {
                command: "foot".into()
            })
        );
    }

    #[test]
    fn workspace_actions_accept_spatial_directions() {
        assert_eq!(
            Action::parse("workspace up"),
            Ok(Action::WorkspaceNavigate {
                direction: Direction::Up,
            })
        );
        assert_eq!(
            Action::parse("move-workspace down"),
            Ok(Action::MoveToWorkspace {
                direction: Direction::Down,
            })
        );
        assert!(Action::parse("workspace north").is_err());
    }

    #[test]
    fn output_actions_require_a_direction() {
        assert_eq!(
            Action::parse("output right"),
            Ok(Action::OutputFocus {
                direction: Direction::Right,
            })
        );
        assert_eq!(
            Action::parse("move-output left"),
            Ok(Action::MoveToOutput {
                direction: Direction::Left,
            })
        );
        assert_eq!(
            Action::parse("workspace-output up"),
            Ok(Action::WorkspaceToOutput {
                direction: Direction::Up,
            })
        );
        assert!(Action::parse("output").is_err());
        assert!(Action::parse("output sideways").is_err());
    }

    #[test]
    fn profile_actions_need_exactly_one_name() {
        assert_eq!(
            Action::parse("profile media"),
            Ok(Action::Profile {
                name: "media".into()
            })
        );
        assert!(Action::parse("profile").is_err());
        assert!(Action::parse("profile two names").is_err());
    }

    #[test]
    fn floating_resize_actions_round_trip_and_reject_invalid_deltas() {
        let action = Action::Resize {
            axis: ResizeAxis::Width,
            delta: -32,
        };
        assert_eq!(Action::parse("resize width -32"), Ok(action.clone()));
        assert_eq!(action.to_string(), "resize width -32");
        assert!(Action::parse("resize width 0").is_err());
        assert!(Action::parse("resize depth 20").is_err());
    }

    #[test]
    fn floating_move_actions_reject_nonpositive_distances() {
        assert_eq!(
            Action::parse("move-floating left 24"),
            Ok(Action::MoveFloating {
                direction: Direction::Left,
                delta: 24,
            })
        );
        assert!(Action::parse("move-floating right 0").is_err());
        assert!(Action::parse("move-floating down -12").is_err());
    }

    #[test]
    fn layout_actions_accept_names_next_and_aliases() {
        assert_eq!(
            Action::parse("layout master-stack"),
            Ok(Action::Layout {
                kind: LayoutKind::MasterStack
            })
        );
        assert_eq!(
            Action::parse("layout scrolling"),
            Ok(Action::Layout {
                kind: LayoutKind::Scroll
            })
        );
        assert_eq!(Action::parse("layout next"), Ok(Action::LayoutNext));
        assert!(Action::parse("layout spiral-ish").is_err());
        assert!(Action::parse("scratchpad now").is_err());
    }

    #[test]
    fn rejects_malformed_actions() {
        for text in [
            "",
            "focus",
            "focus sideways",
            "workspace 0",
            "spawn",
            "quit now",
            "nope",
        ] {
            assert!(Action::parse(text).is_err(), "should reject '{text}'");
        }
    }

    #[test]
    fn serde_representation_matches_the_documented_shape() {
        let action = Action::Focus {
            direction: Direction::Right,
        };
        let json = serde_json::to_string(&action).unwrap_or_default();
        assert_eq!(json, "{\"action\":\"focus\",\"direction\":\"right\"}");
        assert_eq!(
            serde_json::from_str::<Action>("{\"action\":\"quit\"}").ok(),
            Some(Action::Quit)
        );
    }
}
