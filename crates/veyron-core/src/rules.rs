//! Per-application window rules.
//!
//! A rule matches a window by `app_id` and/or title and contributes policy:
//! floating, opacity, blur, gaps, scratchpad, workspace and layout. Rules are
//! evaluated in file order and a later match overrides an earlier one per
//! field, so a broad rule can be specialised by a narrower one below it.
//!
//! Matching uses a small glob language (`*`, `?`) implemented here instead of a
//! regex dependency: the core keeps only `serde`, `toml` and `thiserror`.

use serde::{Deserialize, Serialize};

use crate::tiling::LayoutKind;

/// A glob pattern matched against window metadata.
///
/// `*` matches any run of characters (including none), `?` matches exactly one
/// character. Everything else is matched literally, so `firefox` only matches
/// that exact app id and `firefox*` also matches `firefox-nightly`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Glob {
    pattern: String,
}

impl Glob {
    pub fn new(pattern: impl Into<String>) -> Result<Self, String> {
        let pattern = pattern.into();
        if pattern.trim().is_empty() {
            return Err("pattern cannot be empty".to_string());
        }
        if pattern.len() > 256 {
            return Err("pattern cannot be longer than 256 bytes".to_string());
        }
        Ok(Self { pattern })
    }

    pub fn as_str(&self) -> &str {
        &self.pattern
    }

    /// Match `value` against the pattern. Case-sensitive, like app ids.
    pub fn matches(&self, value: &str) -> bool {
        let pattern: Vec<char> = self.pattern.chars().collect();
        let value: Vec<char> = value.chars().collect();
        let (mut p, mut v) = (0_usize, 0_usize);
        // Last `*` seen and the input position it could resume from.
        let mut star: Option<(usize, usize)> = None;

        while v < value.len() {
            if p < pattern.len() && (pattern[p] == '?' || pattern[p] == value[v]) {
                p += 1;
                v += 1;
            } else if p < pattern.len() && pattern[p] == '*' {
                star = Some((p, v));
                p += 1;
            } else if let Some((star_p, star_v)) = star {
                // Let the last `*` swallow one more character and retry.
                p = star_p + 1;
                v = star_v + 1;
                star = Some((star_p, star_v + 1));
            } else {
                return false;
            }
        }

        while p < pattern.len() && pattern[p] == '*' {
            p += 1;
        }
        p == pattern.len()
    }
}

impl TryFrom<String> for Glob {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Glob::new(value)
    }
}

impl From<Glob> for String {
    fn from(value: Glob) -> Self {
        value.pattern
    }
}

impl std::fmt::Display for Glob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.pattern)
    }
}

/// One `[[rules]]` entry from the configuration file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowRule {
    /// Glob matched against the window's app id.
    pub app_id: Option<Glob>,
    /// Glob matched against the window title.
    pub title: Option<Glob>,
    /// Open the window floating instead of tiled.
    pub floating: Option<bool>,
    /// Per-window opacity in `0.0..=1.0`.
    pub opacity: Option<f32>,
    /// Ask the renderer to blur behind this window.
    pub blur: Option<bool>,
    /// Keep the window in the scratchpad layer instead of the workspace.
    pub scratchpad: Option<bool>,
    /// Named workspace the window belongs to.
    pub workspace: Option<String>,
    /// Layout applied to that workspace when the window is placed.
    pub layout: Option<LayoutKind>,
}

impl WindowRule {
    /// Rules without a match key would apply to every window; that is almost
    /// always a configuration mistake, so it is rejected.
    pub fn validate(&self) -> Result<(), String> {
        if self.app_id.is_none() && self.title.is_none() {
            return Err("a window rule needs app_id and/or title".to_string());
        }
        if let Some(opacity) = self.opacity {
            if !(0.0..=1.0).contains(&opacity) {
                return Err(format!("rule opacity must be in [0, 1] (got {opacity})"));
            }
        }
        if let Some(workspace) = &self.workspace {
            if workspace.trim().is_empty() {
                return Err("rule workspace cannot be empty".to_string());
            }
            if workspace.len() > 64 {
                return Err("rule workspace cannot be longer than 64 bytes".to_string());
            }
        }
        Ok(())
    }

    pub fn matches(&self, app_id: &str, title: &str) -> bool {
        let app_ok = self
            .app_id
            .as_ref()
            .map(|pattern| pattern.matches(app_id))
            .unwrap_or(true);
        let title_ok = self
            .title
            .as_ref()
            .map(|pattern| pattern.matches(title))
            .unwrap_or(true);
        app_ok && title_ok
    }
}

/// Policy resolved from every matching rule.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowPolicy {
    pub floating: bool,
    pub opacity: f32,
    pub blur: bool,
    pub scratchpad: bool,
    pub workspace: Option<String>,
    pub layout: Option<LayoutKind>,
}

impl Default for WindowPolicy {
    fn default() -> Self {
        Self {
            floating: false,
            opacity: 1.0,
            blur: false,
            scratchpad: false,
            workspace: None,
            layout: None,
        }
    }
}

/// The ordered rule list from the configuration file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuleSet {
    rules: Vec<WindowRule>,
}

impl RuleSet {
    pub fn new(rules: Vec<WindowRule>) -> Result<Self, String> {
        for rule in &rules {
            rule.validate()?;
        }
        Ok(Self { rules })
    }

    pub fn rules(&self) -> &[WindowRule] {
        &self.rules
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Resolve the policy for a window. Later rules win per field.
    pub fn resolve(&self, app_id: &str, title: &str) -> WindowPolicy {
        let mut policy = WindowPolicy::default();
        for rule in &self.rules {
            if !rule.matches(app_id, title) {
                continue;
            }
            if let Some(floating) = rule.floating {
                policy.floating = floating;
            }
            if let Some(opacity) = rule.opacity {
                policy.opacity = opacity;
            }
            if let Some(blur) = rule.blur {
                policy.blur = blur;
            }
            if let Some(scratchpad) = rule.scratchpad {
                policy.scratchpad = scratchpad;
            }
            if let Some(workspace) = &rule.workspace {
                policy.workspace = Some(workspace.clone());
            }
            if let Some(layout) = rule.layout {
                policy.layout = Some(layout);
            }
        }
        policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glob(pattern: &str) -> Glob {
        Glob::new(pattern).expect("valid glob")
    }

    #[test]
    fn glob_matches_literally_and_with_wildcards() {
        assert!(glob("firefox").matches("firefox"));
        assert!(!glob("firefox").matches("firefox-nightly"));
        assert!(glob("firefox*").matches("firefox-nightly"));
        assert!(glob("*firefox*").matches("org.mozilla.firefox"));
        assert!(glob("org.*.firefox").matches("org.mozilla.firefox"));
        assert!(!glob("org.*.firefox").matches("org.mozilla.thunderbird"));
        assert!(glob("?").matches("a"));
        assert!(!glob("?").matches("ab"));
        assert!(glob("*").matches(""));
        assert!(glob("a*b*c").matches("aXXbYYc"));
        assert!(!glob("a*b*c").matches("aXXbYY"));
        assert!(glob("*.*").matches(".config"));
    }

    #[test]
    fn pattern_length_and_emptiness_are_validated() {
        assert!(Glob::new("").is_err());
        assert!(Glob::new("   ").is_err());
        assert!(Glob::new("x".repeat(257)).is_err());
        assert!(Glob::new("x".repeat(256)).is_ok());
    }

    #[test]
    fn serde_parses_globs_from_plain_strings() {
        let rule: WindowRule =
            toml::from_str("app_id = \"foot*\"\nfloating = true\nopacity = 0.9\n")
                .expect("valid rule");
        assert_eq!(rule.app_id.as_ref().map(Glob::as_str), Some("foot*"));
        assert_eq!(rule.floating, Some(true));
        assert!(rule.matches("foot-client", "anything"));
        assert!(!rule.matches("alacritty", "anything"));
    }

    #[test]
    fn rules_need_a_match_key() {
        let rule = WindowRule {
            floating: Some(true),
            ..WindowRule::default()
        };
        assert!(rule.validate().is_err());
        assert!(RuleSet::new(vec![rule]).is_err());

        let bad_opacity = WindowRule {
            app_id: Some(glob("x")),
            opacity: Some(1.5),
            ..WindowRule::default()
        };
        assert!(bad_opacity.validate().is_err());
    }

    #[test]
    fn later_rules_override_earlier_ones_per_field() {
        let rules = RuleSet::new(vec![
            WindowRule {
                app_id: Some(glob("org.mozilla.*")),
                floating: Some(true),
                opacity: Some(0.9),
                blur: Some(true),
                ..WindowRule::default()
            },
            WindowRule {
                app_id: Some(glob("org.mozilla.firefox")),
                floating: Some(false),
                title: Some(glob("Picture-in-Picture")),
                opacity: Some(0.7),
                ..WindowRule::default()
            },
        ])
        .expect("valid rules");

        let policy = rules.resolve("org.mozilla.firefox", "Picture-in-Picture");
        assert!(!policy.floating, "the narrower rule wins");
        assert_eq!(policy.opacity, 0.7);
        assert!(policy.blur, "untouched fields keep the earlier value");

        let other = rules.resolve("org.mozilla.firefox", "Mozilla Firefox");
        assert!(other.floating);
        assert_eq!(other.opacity, 0.9);
    }

    #[test]
    fn workspace_and_layout_rules_are_resolved() {
        let rules = RuleSet::new(vec![WindowRule {
            app_id: Some(glob("signal")),
            workspace: Some("chat".into()),
            layout: Some(LayoutKind::Dwindle),
            scratchpad: Some(false),
            ..WindowRule::default()
        }])
        .expect("valid rules");

        let policy = rules.resolve("signal", "");
        assert_eq!(policy.workspace.as_deref(), Some("chat"));
        assert_eq!(policy.layout, Some(LayoutKind::Dwindle));
        assert!(!policy.scratchpad);

        let default = rules.resolve("foot", "");
        assert_eq!(default, WindowPolicy::default());
    }
}
