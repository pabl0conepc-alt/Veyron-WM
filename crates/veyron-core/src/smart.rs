//! Placement heuristics that make the workspace remember where things belong.
//!
//! Rules stay authoritative: they are matched by app id and title and are the
//! only way to *pin* a window. What lives here is learned behaviour — where an
//! application's windows actually ended up — so a new window of that
//! application can open next to its siblings instead of on whatever workspace
//! happens to be active.

use std::collections::HashMap;

/// Where an application's windows last lived.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Affinity {
    /// Output name the window was placed on.
    pub output: String,
    /// Workspace label it was placed on.
    pub workspace: String,
}

/// Learned `app id -> output/workspace` map.
///
/// Learning is unconditional (it is cheap and keeps the history warm across a
/// configuration reload); whether the map is *used* is decided by the caller.
#[derive(Debug, Clone, Default)]
pub struct Affinities {
    entries: HashMap<String, Affinity>,
}

impl Affinities {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record where a window of `app_id` was last seen. Anonymous windows (the
    /// state between a toplevel appearing and its client announcing an app id)
    /// are ignored so they cannot evict a real entry with an empty key.
    pub fn observe(&mut self, app_id: &str, output: &str, workspace: &str) {
        if app_id.is_empty() || output.is_empty() || workspace.is_empty() {
            return;
        }
        self.entries.insert(
            app_id.to_string(),
            Affinity {
                output: output.to_string(),
                workspace: workspace.to_string(),
            },
        );
    }

    pub fn get(&self, app_id: &str) -> Option<&Affinity> {
        self.entries.get(app_id)
    }

    /// Drop entries whose output or workspace no longer exists, so a removed
    /// workspace cannot keep attracting windows.
    pub fn retain_workspaces(&mut self, mut keep: impl FnMut(&str, &str) -> bool) {
        self.entries
            .retain(|_, affinity| keep(&affinity.output, &affinity.workspace));
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learning_records_the_latest_placement() {
        let mut affinities = Affinities::new();
        assert!(affinities.is_empty());

        affinities.observe("firefox", "DP-1", "web");
        assert_eq!(
            affinities.get("firefox"),
            Some(&Affinity {
                output: "DP-1".into(),
                workspace: "web".into()
            })
        );

        affinities.observe("firefox", "eDP-1", "2");
        assert_eq!(
            affinities
                .get("firefox")
                .map(|affinity| affinity.workspace.as_str()),
            Some("2")
        );
        assert_eq!(affinities.len(), 1);
    }

    #[test]
    fn anonymous_windows_never_become_an_entry() {
        let mut affinities = Affinities::new();
        affinities.observe("", "DP-1", "1");
        affinities.observe("mpv", "", "1");
        affinities.observe("mpv", "DP-1", "");
        assert!(affinities.is_empty());
        assert_eq!(affinities.get("mpv"), None);
    }

    #[test]
    fn pruning_follows_workspace_lifecycle() {
        let mut affinities = Affinities::new();
        affinities.observe("firefox", "DP-1", "web");
        affinities.observe("mpv", "DP-1", "media");

        affinities.retain_workspaces(|output, workspace| output == "DP-1" && workspace == "web");
        assert!(affinities.get("firefox").is_some());
        assert_eq!(affinities.get("mpv"), None);

        affinities.clear();
        assert!(affinities.is_empty());
    }
}
