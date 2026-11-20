//! Veyron Recall — session capture and restore.
//!
//! A session is plain serialisable data: the workspaces that existed and, for
//! every window, the application id, title, workspace, tiling mode and floating
//! geometry. Capturing it never touches the filesystem and restoring it goes
//! through the same [`crate::wm::Wm`] APIs a real client would use, so the
//! feature can be tested end to end without a compositor.
//!
//! Restoring relaunches nothing: the compositor (or the user's startup commands)
//! starts the applications, and Recall puts each matching window back where it
//! was as it appears.

use serde::{Deserialize, Serialize};

use crate::{
    geometry::Rect,
    window::WindowId,
    wm::{Snapshot, Wm},
};

/// Largest session that will be loaded, a guard against a corrupted or hostile
/// file asking for an unbounded number of windows.
pub const MAX_SESSION_WINDOWS: usize = 4096;

/// One remembered window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowRecord {
    pub app_id: String,
    #[serde(default)]
    pub title: String,
    pub workspace: String,
    #[serde(default)]
    pub floating: bool,
    #[serde(default)]
    pub scratchpad: bool,
    /// Floating geometry, when the window was floating.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<Rect>,
}

impl WindowRecord {
    pub fn validate(&self) -> Result<(), String> {
        if self.app_id.len() > 256 {
            return Err("session app_id cannot be longer than 256 bytes".to_string());
        }
        if self.title.len() > 1024 {
            return Err("session title cannot be longer than 1024 bytes".to_string());
        }
        if self.workspace.trim().is_empty() {
            return Err("session workspace cannot be empty".to_string());
        }
        if self.workspace.len() > 64 {
            return Err("session workspace cannot be longer than 64 bytes".to_string());
        }
        if let Some(rect) = self.rect {
            if rect.w < 0 || rect.h < 0 {
                return Err("session window geometry cannot be negative".to_string());
            }
        }
        Ok(())
    }
}

/// A captured session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Session {
    /// Workspace names, in creation order, including empty ones.
    pub workspaces: Vec<String>,
    pub windows: Vec<WindowRecord>,
}

impl Session {
    /// Capture the state of a running window manager.
    pub fn capture(wm: &Wm) -> Session {
        let mut session = Session::default();
        // Only the active output is captured: Recall restores onto a single
        // output, and taking every output would collide on shared workspace
        // names such as "1".
        let output = wm.active_output();
        for workspace in output.workspaces() {
            session.workspaces.push(workspace.name().to_string());
            for id in workspace.layout().order() {
                push_record(&mut session, wm, id, workspace.name(), false);
            }
            for id in workspace.floating_windows() {
                push_record(&mut session, wm, *id, workspace.name(), false);
            }
        }

        let active = wm.active_output().active_workspace().name().to_string();
        for id in wm.scratchpad_windows() {
            push_record(&mut session, wm, *id, &active, true);
        }
        session
    }

    /// Windows recorded on a given workspace.
    pub fn windows_on<'a>(
        &'a self,
        workspace: &'a str,
    ) -> impl Iterator<Item = &'a WindowRecord> + 'a {
        self.windows
            .iter()
            .filter(move |record| record.workspace == workspace)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.windows.len() > MAX_SESSION_WINDOWS {
            return Err(format!(
                "session has too many windows ({} > {MAX_SESSION_WINDOWS})",
                self.windows.len()
            ));
        }
        for workspace in &self.workspaces {
            if workspace.trim().is_empty() {
                return Err("session workspace name cannot be empty".to_string());
            }
            if workspace.len() > 64 {
                return Err("session workspace name cannot be longer than 64 bytes".to_string());
            }
        }
        for record in &self.windows {
            record.validate()?;
        }
        Ok(())
    }

    pub fn to_toml(&self) -> Result<String, String> {
        self.validate()?;
        toml::to_string_pretty(self).map_err(|error| format!("could not encode session: {error}"))
    }

    pub fn from_toml(text: &str) -> Result<Session, String> {
        let session: Session =
            toml::from_str(text).map_err(|error| format!("invalid session: {error}"))?;
        session.validate()?;
        Ok(session)
    }

    /// Recreate every recorded window on a window manager, returning how many
    /// were restored. The window manager is expected to be freshly built (or at
    /// least not to already hold these windows): Recall never deletes anything.
    pub fn restore(&self, wm: &mut Wm) -> usize {
        wm.restore_session(self)
    }
}

fn push_record(session: &mut Session, wm: &Wm, id: WindowId, workspace: &str, scratchpad: bool) {
    let Some(window) = wm.window(id) else {
        return;
    };
    let floating = window.is_floating();
    session.windows.push(WindowRecord {
        app_id: window.app_id().to_string(),
        title: window.title().to_string(),
        workspace: workspace.to_string(),
        floating,
        scratchpad,
        rect: floating.then(|| window.floating_rect()),
    });
}

/// Workspace the compositor currently shows, useful when recording a session
/// from the outside (for example over IPC).
pub fn active_workspace(snapshot: &Snapshot) -> &str {
    &snapshot.workspace
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, geometry::Size, window::NewWindow};

    fn wm() -> Wm {
        Wm::new(Config::default(), "VEYRON-TEST", Size::new(1280, 800))
    }

    fn open(wm: &mut Wm, app_id: &str, title: &str) -> WindowId {
        wm.create_window(NewWindow {
            app_id: app_id.to_string(),
            title: title.to_string(),
            min_size: None,
        })
    }

    #[test]
    fn capture_records_workspaces_windows_and_geometry() {
        let mut wm = wm();
        open(&mut wm, "foot", "Terminal");
        wm.dispatch(crate::action::Action::Workspace { index: 2 });
        let browser = open(&mut wm, "firefox", "Start Page");
        wm.dispatch(crate::action::Action::ToggleFloating);
        let rect = wm
            .snapshot()
            .placement_of(browser)
            .map(|placement| placement.rect)
            .expect("floating window is placed");

        let session = Session::capture(&wm);
        assert_eq!(session.workspaces, vec!["1".to_string(), "2".to_string()]);
        assert_eq!(session.windows.len(), 2);
        assert_eq!(session.windows[0].app_id, "foot");
        assert_eq!(session.windows[0].workspace, "1");
        assert!(!session.windows[0].floating);
        assert_eq!(session.windows[1].app_id, "firefox");
        assert_eq!(session.windows[1].workspace, "2");
        assert!(session.windows[1].floating);
        assert_eq!(session.windows[1].rect, Some(rect));
    }

    #[test]
    fn capture_records_scratchpad_windows_once() {
        let mut wm = wm();
        let parked = open(&mut wm, "pavucontrol", "Volume");
        wm.dispatch(crate::action::Action::ToggleScratchpad);

        let session = Session::capture(&wm);
        assert_eq!(session.windows.len(), 1);
        assert_eq!(session.windows[0].app_id, "pavucontrol");
        assert!(session.windows[0].scratchpad);
        assert!(wm.scratchpad_windows().contains(&parked));
    }

    #[test]
    fn session_text_round_trips() {
        let session = Session {
            workspaces: vec!["1".into(), "chat".into()],
            windows: vec![
                WindowRecord {
                    app_id: "foot".into(),
                    title: "Terminal".into(),
                    workspace: "1".into(),
                    floating: false,
                    scratchpad: false,
                    rect: None,
                },
                WindowRecord {
                    app_id: "firefox".into(),
                    title: "Picture-in-Picture".into(),
                    workspace: "chat".into(),
                    floating: true,
                    scratchpad: false,
                    rect: Some(Rect::new(10, 20, 640, 360)),
                },
            ],
        };

        let text = session.to_toml().expect("session encodes");
        assert_eq!(Session::from_toml(&text), Ok(session));
    }

    #[test]
    fn restore_recreates_windows_in_their_workspaces() {
        let mut first = wm();
        open(&mut first, "foot", "Terminal");
        first.dispatch(crate::action::Action::Workspace { index: 3 });
        open(&mut first, "firefox", "Browser");
        first.dispatch(crate::action::Action::ToggleFloating);
        let floating = first
            .snapshot()
            .placements
            .iter()
            .find(|placement| placement.floating)
            .map(|placement| placement.rect)
            .expect("a floating window");

        let text = Session::capture(&first).to_toml().expect("session encodes");
        let session = Session::from_toml(&text).expect("session decodes");

        let mut second = wm();
        assert_eq!(session.restore(&mut second), 2);
        assert_eq!(second.window_count(), 2);
        assert_eq!(second.workspace_count(), 3);

        let restored = Session::capture(&second);
        assert_eq!(restored.workspaces, session.workspaces);
        assert_eq!(restored.windows.len(), session.windows.len());

        let browser = second
            .windows()
            .find(|window| window.app_id() == "firefox")
            .expect("the browser was restored");
        assert!(browser.is_floating());
        assert_eq!(Some(browser.floating_rect()), Some(floating));
    }

    #[test]
    fn invalid_sessions_are_rejected() {
        assert!(Session::from_toml("windows = 3").is_err());
        assert!(Session::from_toml("unknown = 1").is_err());

        let session = Session {
            workspaces: vec!["  ".into()],
            windows: Vec::new(),
        };
        assert!(session.to_toml().is_err());

        let bad = Session {
            workspaces: Vec::new(),
            windows: vec![WindowRecord {
                app_id: "foot".into(),
                title: String::new(),
                workspace: "1".into(),
                floating: true,
                scratchpad: false,
                rect: Some(Rect {
                    x: 0,
                    y: 0,
                    w: -4,
                    h: 10,
                }),
            }],
        };
        assert!(bad.validate().is_err());
    }
}
