//! Window groups.
//!
//! A group is an ordered set of windows the user wants to treat as a unit:
//! members are kept next to each other in the strip and focus cycles inside the
//! group with one keybinding. Membership is exclusive — a window belongs to at
//! most one group — so the grouping state is always unambiguous.
//!
//! The set owns no geometry: it tracks membership and order, and the layout is
//! reordered through [`crate::layout::ScrollLayout::reorder`].

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::window::WindowId;

/// Stable identifier of a group, allocated by [`GroupSet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GroupId(pub u32);

impl std::fmt::Display for GroupId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// One group and its members, in strip order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowGroup {
    id: GroupId,
    name: String,
    members: Vec<WindowId>,
}

impl WindowGroup {
    pub fn id(&self) -> GroupId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn members(&self) -> &[WindowId] {
        &self.members
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

/// Every group of a session.
#[derive(Debug, Clone, Default)]
pub struct GroupSet {
    groups: Vec<WindowGroup>,
    next_id: u32,
}

impl GroupSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn groups(&self) -> &[WindowGroup] {
        &self.groups
    }

    pub fn len(&self) -> usize {
        self.groups.len()
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    pub fn group(&self, id: GroupId) -> Option<&WindowGroup> {
        self.groups.iter().find(|group| group.id == id)
    }

    /// The group a window belongs to, if any.
    pub fn group_of(&self, window: WindowId) -> Option<GroupId> {
        self.groups
            .iter()
            .find(|group| group.members.contains(&window))
            .map(|group| group.id)
    }

    /// Members of the window's group, or an empty slice when it is ungrouped.
    pub fn members_of(&self, window: WindowId) -> &[WindowId] {
        self.groups
            .iter()
            .find(|group| group.members.contains(&window))
            .map(|group| group.members.as_slice())
            .unwrap_or(&[])
    }

    /// Add `window` to the group of `with`, or start a new group with it.
    ///
    /// Joining always moves the window out of its previous group, so membership
    /// can never contradict itself.
    pub fn join(&mut self, window: WindowId, with: Option<WindowId>) -> GroupId {
        let existing = self.group_of(window);

        if let Some(id) = with.and_then(|other| self.group_of(other)) {
            if existing == Some(id) {
                return id;
            }
            self.leave(window);
            if let Some(group) = self.groups.iter_mut().find(|group| group.id == id) {
                group.members.push(window);
                return id;
            }
            // The target group only held `window`, so leaving dropped it.
        }

        if let Some(id) = existing {
            return id;
        }

        // New group. A still-ungrouped partner joins it, so grouping two loose
        // windows always produces a group of two.
        self.next_id += 1;
        let id = GroupId(self.next_id);
        let members = match with {
            Some(other) if other != window => vec![other, window],
            _ => vec![window],
        };
        self.groups.push(WindowGroup {
            id,
            name: format!("group {}", id.0),
            members,
        });
        id
    }

    /// Remove a window from its group, dropping the group when it empties.
    pub fn leave(&mut self, window: WindowId) -> Option<GroupId> {
        let index = self
            .groups
            .iter()
            .position(|group| group.members.contains(&window))?;
        let group = &mut self.groups[index];
        group.members.retain(|member| *member != window);
        let id = group.id;
        if group.members.is_empty() {
            self.groups.remove(index);
        }
        Some(id)
    }

    /// Group the window when it is ungrouped, ungroup it when it is grouped.
    /// Returns the group it ends up in, if any.
    pub fn toggle(&mut self, window: WindowId, with: Option<WindowId>) -> Option<GroupId> {
        if self.group_of(window).is_some() {
            self.leave(window);
            None
        } else {
            Some(self.join(window, with))
        }
    }

    /// Next (`delta > 0`) or previous (`delta < 0`) member of the window's
    /// group, wrapping around. `None` when the window is alone in its group.
    pub fn cycle(&self, window: WindowId, delta: isize) -> Option<WindowId> {
        let members = self.members_of(window);
        if members.len() < 2 {
            return None;
        }
        let index = members.iter().position(|member| *member == window)? as isize;
        let target = (index + delta).rem_euclid(members.len() as isize) as usize;
        members.get(target).copied()
    }

    /// Forget windows that no longer exist and drop groups left empty.
    pub fn prune(&mut self, alive: &HashSet<WindowId>) {
        for group in &mut self.groups {
            group.members.retain(|member| alive.contains(member));
        }
        self.groups.retain(|group| !group.members.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: u64) -> WindowId {
        WindowId(id)
    }

    #[test]
    fn joining_creates_a_group_and_extends_the_partners_group() {
        let mut groups = GroupSet::new();
        let first = groups.join(window(1), None);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups.members_of(window(1)), &[window(1)]);
        assert_eq!(groups.group(first).map(WindowGroup::name), Some("group 1"));

        let same = groups.join(window(2), Some(window(1)));
        assert_eq!(same, first);
        assert_eq!(groups.members_of(window(1)), &[window(1), window(2)]);

        let other = groups.join(window(3), None);
        assert_ne!(other, first);
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn a_window_belongs_to_at_most_one_group() {
        let mut groups = GroupSet::new();
        groups.join(window(1), None);
        groups.join(window(2), None);
        let second = groups.group_of(window(2));

        groups.join(window(2), Some(window(1)));
        assert_eq!(groups.group_of(window(2)), groups.group_of(window(1)));
        assert_ne!(groups.group_of(window(2)), second);
        assert_eq!(
            groups.len(),
            1,
            "the emptied group is dropped, not left dangling"
        );
    }

    #[test]
    fn leaving_removes_the_window_and_drops_empty_groups() {
        let mut groups = GroupSet::new();
        groups.join(window(1), None);
        groups.join(window(2), Some(window(1)));

        assert!(groups.leave(window(2)).is_some());
        assert_eq!(groups.members_of(window(1)), &[window(1)]);
        assert!(groups.leave(window(1)).is_some());
        assert!(groups.is_empty());
        assert!(groups.leave(window(9)).is_none());
    }

    #[test]
    fn toggle_groups_and_ungroups() {
        let mut groups = GroupSet::new();
        assert!(groups.toggle(window(1), None).is_some());
        assert!(groups.group_of(window(1)).is_some());
        assert!(groups.toggle(window(1), None).is_none());
        assert!(groups.group_of(window(1)).is_none());
        assert!(groups.is_empty());
    }

    #[test]
    fn cycling_wraps_and_ignores_lone_windows() {
        let mut groups = GroupSet::new();
        groups.join(window(1), None);
        groups.join(window(2), Some(window(1)));
        groups.join(window(3), Some(window(1)));
        assert!(groups.group_of(window(3)).is_some());

        assert_eq!(groups.cycle(window(1), 1), Some(window(2)));
        assert_eq!(groups.cycle(window(2), 1), Some(window(3)));
        assert_eq!(groups.cycle(window(3), 1), Some(window(1)));
        assert_eq!(groups.cycle(window(1), -1), Some(window(3)));
        assert_eq!(groups.cycle(window(7), 1), None);

        let mut lone = GroupSet::new();
        lone.join(window(1), None);
        assert_eq!(lone.cycle(window(1), 1), None);
    }

    #[test]
    fn pruning_forgets_closed_windows() {
        let mut groups = GroupSet::new();
        groups.join(window(1), None);
        groups.join(window(2), Some(window(1)));
        groups.join(window(3), None);

        let alive = HashSet::from([window(1), window(3)]);
        groups.prune(&alive);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups.members_of(window(1)), &[window(1)]);
        assert!(groups.group_of(window(2)).is_none());

        groups.prune(&HashSet::new());
        assert!(groups.is_empty());
    }
}
