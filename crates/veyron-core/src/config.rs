//! Configuration model, parser and validation.
//!
//! Veyron uses TOML because it is easy to read and merge, and because a typo in
//! an unknown key is reported as an error instead of being silently ignored.
//! Every value is validated before the compositor starts using it.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

use crate::{
    action::{Action, ActionParseError},
    layout::LayoutConfig,
    motion::MotionConfig,
    profiles::{Profile, Profiles},
    rules::{RuleSet, WindowRule},
    theme::ThemeConfig,
    tiling::LayoutKind,
};

/// Modifiers that can be part of a keybinding chord.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub logo: bool,
}

impl Modifiers {
    /// Case-insensitive match used by the input layer.
    pub fn matches(&self, ctrl: bool, alt: bool, shift: bool, logo: bool) -> bool {
        self.ctrl == ctrl && self.alt == alt && self.shift == shift && self.logo == logo
    }
}

/// A named workspace that exists as soon as the compositor starts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspacePreset {
    /// Label also used by `workspace <name>` and window rules.
    pub name: String,
    /// Sparse grid coordinate. Set both `x` and `y`, or neither.
    pub x: Option<i64>,
    pub y: Option<i64>,
    /// Layout selected when the workspace is created.
    pub layout: Option<LayoutKind>,
}

impl WorkspacePreset {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("workspace name cannot be empty".to_string());
        }
        if self.name.len() > 64 {
            return Err("workspace name cannot be longer than 64 bytes".to_string());
        }
        if self.x.is_some() != self.y.is_some() {
            return Err(format!(
                "workspace '{}' needs both x and y, or neither",
                self.name
            ));
        }
        Ok(())
    }

    /// Pinned grid position, when the preset sets one.
    pub fn position(&self) -> Option<(i64, i64)> {
        self.x.zip(self.y)
    }
}

/// A chord bound to an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub modifiers: Modifiers,
    /// Lowercase keysym name, e.g. `return`, `left`, `1`.
    pub key: String,
    pub action: Action,
}

impl Binding {
    /// Parse a chord such as `super+shift+left`.
    ///
    /// The last `+`-separated token is the key, so names that need a literal
    /// `+` use `plus`.
    pub fn parse(chord: &str, action: Action) -> Result<Binding, ConfigError> {
        let mut parts: Vec<&str> = chord
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect();

        let key = parts
            .pop()
            .ok_or_else(|| ConfigError::Invalid(format!("empty keybinding chord '{chord}'")))?
            .to_ascii_lowercase();

        let mut modifiers = Modifiers::default();
        for part in parts {
            match part.to_ascii_lowercase().as_str() {
                "super" | "logo" | "mod" | "win" | "meta" => modifiers.logo = true,
                "ctrl" | "control" => modifiers.ctrl = true,
                "alt" | "mod1" => modifiers.alt = true,
                "shift" => modifiers.shift = true,
                other => {
                    return Err(ConfigError::UnknownModifier {
                        chord: chord.to_string(),
                        modifier: other.to_string(),
                    })
                }
            }
        }

        if key.is_empty() {
            return Err(ConfigError::Invalid(format!(
                "empty key in keybinding chord '{chord}'"
            )));
        }

        Ok(Binding {
            modifiers,
            key,
            action,
        })
    }

    /// Canonical chord text, used when writing a configuration file back out.
    pub fn chord(&self) -> String {
        let mut parts = Vec::new();
        if self.modifiers.logo {
            parts.push("super");
        }
        if self.modifiers.ctrl {
            parts.push("ctrl");
        }
        if self.modifiers.alt {
            parts.push("alt");
        }
        if self.modifiers.shift {
            parts.push("shift");
        }
        parts.push(&self.key);
        parts.join("+")
    }
}

/// Visual configuration (phase 1 keeps the surface intentionally small).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    /// Desktop background colour, `#rgb`, `#rrggbb` or `#rrggbbaa`.
    pub background: String,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            background: "#101014".to_string(),
        }
    }
}

impl Appearance {
    /// Background colour as linear-ish RGBA floats for the renderer.
    /// Falls back to the default colour if the stored value is invalid, which
    /// cannot happen for a config produced through [`Config::from_toml_str`].
    pub fn background_rgba(&self) -> [f32; 4] {
        parse_hex_color(&self.background).unwrap_or([0.063, 0.063, 0.078, 1.0])
    }
}

/// Behaviour that is not visual and not tiling geometry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Behavior {
    /// Remember the last floating geometry per app id and restore it for the
    /// next window of that application.
    pub remember_geometry: bool,
    /// Open a new window on the output and workspace where the same application
    /// already lives. Rules that pin a workspace always win over it.
    pub smart_workspace: bool,
}

impl Default for Behavior {
    fn default() -> Self {
        Self {
            remember_geometry: true,
            smart_workspace: false,
        }
    }
}

/// Fully validated compositor configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub layout: LayoutConfig,
    pub appearance: Appearance,
    /// Material You scheme for borders and exported desktop colours.
    pub theme: ThemeConfig,
    pub motion: MotionConfig,
    pub behavior: Behavior,
    pub rules: RuleSet,
    pub bindings: Vec<Binding>,
    /// Commands started once when the compositor comes up.
    pub startup: Vec<String>,
    /// Named layout/motion presets usable with `profile <name>`.
    pub profiles: Profiles,
    /// Profile applied when the compositor starts.
    pub default_profile: Option<String>,
    /// Named workspaces created at startup on the primary output.
    pub workspaces: Vec<WorkspacePreset>,
}

/// Default chords. Kept in one place so the documentation, the default config
/// and the tests cannot disagree.
const DEFAULT_BINDINGS: &[(&str, &str)] = &[
    ("super+return", "spawn alacritty"),
    ("super+q", "close"),
    ("super+left", "focus left"),
    ("super+right", "focus right"),
    ("super+up", "focus up"),
    ("super+down", "focus down"),
    ("super+shift+left", "move left"),
    ("super+shift+right", "move right"),
    ("super+shift+up", "move up"),
    ("super+shift+down", "move down"),
    ("super+minus", "column-width -10"),
    ("super+equal", "column-width 10"),
    ("super+c", "center-column"),
    ("super+f", "toggle-fullscreen"),
    ("super+space", "toggle-floating"),
    ("super+shift+l", "layout next"),
    ("super+shift+s", "scratchpad"),
    ("super+1", "workspace 1"),
    ("super+2", "workspace 2"),
    ("super+3", "workspace 3"),
    ("super+4", "workspace 4"),
    ("super+5", "workspace 5"),
    ("super+bracketright", "workspace next"),
    ("super+bracketleft", "workspace prev"),
    ("super+ctrl+left", "workspace left"),
    ("super+ctrl+right", "workspace right"),
    ("super+ctrl+up", "workspace up"),
    ("super+ctrl+down", "workspace down"),
    ("super+ctrl+shift+left", "move-workspace left"),
    ("super+ctrl+shift+right", "move-workspace right"),
    ("super+ctrl+shift+up", "move-workspace up"),
    ("super+ctrl+shift+down", "move-workspace down"),
    ("super+alt+left", "resize width -32"),
    ("super+alt+right", "resize width 32"),
    ("super+alt+up", "resize height -32"),
    ("super+alt+down", "resize height 32"),
    ("super+alt+shift+left", "move-floating left 32"),
    ("super+alt+shift+right", "move-floating right 32"),
    ("super+alt+shift+up", "move-floating up 32"),
    ("super+alt+shift+down", "move-floating down 32"),
    ("super+tab", "focus previous"),
    ("super+shift+g", "group"),
    ("super+shift+n", "group next"),
    ("super+shift+p", "pip"),
    ("super+ctrl+alt+left", "snap left"),
    ("super+ctrl+alt+right", "snap right"),
    ("super+ctrl+alt+up", "snap top"),
    ("super+ctrl+alt+down", "snap bottom"),
    ("super+comma", "output left"),
    ("super+period", "output right"),
    ("super+shift+comma", "move-output left"),
    ("super+shift+period", "move-output right"),
    ("super+ctrl+comma", "workspace-output left"),
    ("super+ctrl+period", "workspace-output right"),
    ("super+p", "profile next"),
    ("alt+tab", "focus-cycle next"),
    ("alt+shift+tab", "focus-cycle prev"),
    ("super+shift+1", "move-to-workspace 1"),
    ("super+shift+2", "move-to-workspace 2"),
    ("super+shift+3", "move-to-workspace 3"),
    ("super+shift+4", "move-to-workspace 4"),
    ("super+shift+5", "move-to-workspace 5"),
    ("super+shift+c", "center-floating"),
    ("super+ctrl+minus", "gaps -4"),
    ("super+ctrl+equal", "gaps 4"),
    ("super+alt+minus", "opacity -10"),
    ("super+alt+equal", "opacity 10"),
    ("super+t", "theme toggle"),
    ("super+shift+t", "theme next"),
    ("super+ctrl+t", "theme variant next"),
    ("super+alt+t", "theme design next"),
    ("super+r", "reload"),
    ("super+shift+e", "quit"),
];

impl Default for Config {
    fn default() -> Self {
        let mut config = Self {
            layout: LayoutConfig::default(),
            appearance: Appearance::default(),
            theme: ThemeConfig::default(),
            motion: MotionConfig::default(),
            behavior: Behavior::default(),
            rules: RuleSet::default(),
            bindings: Vec::new(),
            startup: Vec::new(),
            profiles: Profiles::default(),
            default_profile: None,
            workspaces: Vec::new(),
        };
        for (chord, action) in DEFAULT_BINDINGS {
            let binding = Binding::parse(
                chord,
                Action::parse(action).expect("default binding has a valid action"),
            )
            .expect("default binding has a valid chord");
            config.bindings.push(binding);
        }
        config
    }
}

impl Config {
    /// Parse and validate TOML text.
    ///
    /// A missing table means "use the default"; an explicitly present table is
    /// validated strictly. If `[bindings]` is present it replaces the default
    /// bindings as a whole.
    pub fn from_toml_str(text: &str) -> Result<Config, ConfigError> {
        let raw: RawConfig = toml::from_str(text)?;

        let layout = raw.layout.unwrap_or_default();
        layout.validate().map_err(ConfigError::Invalid)?;

        let appearance = raw.appearance.unwrap_or_default();
        if parse_hex_color(&appearance.background).is_none() {
            return Err(ConfigError::InvalidColor(appearance.background));
        }

        let theme = raw.theme.unwrap_or_default();
        theme.validate().map_err(ConfigError::Invalid)?;

        let motion = raw.motion.unwrap_or_default();
        motion.validate().map_err(ConfigError::Invalid)?;

        let rules = RuleSet::new(raw.rules.unwrap_or_default()).map_err(ConfigError::Invalid)?;

        let mut bindings = Vec::new();
        let mut seen: BTreeMap<(bool, bool, bool, bool, String), ()> = BTreeMap::new();
        match raw.bindings {
            Some(table) => {
                for (chord, action) in table {
                    let parsed = Action::parse(&action)?;
                    let binding = Binding::parse(&chord, parsed)?;
                    let key = (
                        binding.modifiers.ctrl,
                        binding.modifiers.alt,
                        binding.modifiers.shift,
                        binding.modifiers.logo,
                        binding.key.clone(),
                    );
                    if seen.insert(key, ()).is_some() {
                        return Err(ConfigError::DuplicateBinding(chord));
                    }
                    bindings.push(binding);
                }
            }
            None => bindings = Config::default().bindings,
        }

        let workspaces = raw.workspaces.unwrap_or_default();
        if workspaces.len() > 32 {
            return Err(ConfigError::Invalid(
                "at most 32 workspaces can be predefined".to_string(),
            ));
        }
        let mut workspace_names = BTreeSet::new();
        for preset in &workspaces {
            preset.validate().map_err(ConfigError::Invalid)?;
            if !workspace_names.insert(preset.name.clone()) {
                return Err(ConfigError::Invalid(format!(
                    "duplicate workspace name '{}'",
                    preset.name
                )));
            }
        }

        let profiles =
            Profiles::new(raw.profiles.unwrap_or_default()).map_err(ConfigError::Invalid)?;
        let default_profile = raw.default_profile;
        if let Some(name) = &default_profile {
            if profiles.get(name).is_none() {
                return Err(ConfigError::Invalid(format!(
                    "default_profile '{name}' is not defined in [[profiles]]"
                )));
            }
        }

        Ok(Config {
            layout,
            appearance,
            theme,
            motion,
            behavior: raw.behavior.unwrap_or_default(),
            rules,
            bindings,
            startup: raw.startup.unwrap_or_default(),
            profiles,
            default_profile,
            workspaces,
        })
    }

    pub fn load(path: &std::path::Path) -> Result<Config, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Config::from_toml_str(&text)
    }

    /// Load the configuration, falling back to defaults when the file is absent.
    /// Other errors (syntax, invalid values, permissions) are reported: silently
    /// ignoring a broken configuration is a security and UX hazard.
    pub fn load_or_default(path: &std::path::Path) -> Result<Config, ConfigError> {
        match Config::load(path) {
            Ok(config) => Ok(config),
            Err(ConfigError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(Config::default())
            }
            Err(error) => Err(error),
        }
    }

    /// `$XDG_CONFIG_HOME/veyron/veyron.toml`, or the home-directory equivalent.
    pub fn default_path() -> PathBuf {
        if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
            return PathBuf::from(dir).join("veyron").join("veyron.toml");
        }
        if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
            return PathBuf::from(home)
                .join(".config")
                .join("veyron")
                .join("veyron.toml");
        }
        PathBuf::from("veyron.toml")
    }

    /// Render the configuration back to TOML, used to seed a user config file.
    pub fn to_toml_string(&self) -> Result<String, ConfigError> {
        let mut bindings = BTreeMap::new();
        for binding in &self.bindings {
            bindings.insert(binding.chord(), binding.action.to_string());
        }

        #[derive(Serialize)]
        struct Serializable<'a> {
            layout: &'a LayoutConfig,
            appearance: &'a Appearance,
            theme: &'a ThemeConfig,
            motion: &'a MotionConfig,
            behavior: &'a Behavior,
            default_profile: &'a Option<String>,
            rules: &'a [WindowRule],
            bindings: BTreeMap<String, String>,
            startup: &'a Vec<String>,
            profiles: &'a [Profile],
            workspaces: &'a [WorkspacePreset],
        }

        let serializable = Serializable {
            layout: &self.layout,
            appearance: &self.appearance,
            theme: &self.theme,
            motion: &self.motion,
            behavior: &self.behavior,
            default_profile: &self.default_profile,
            rules: self.rules.rules(),
            bindings,
            startup: &self.startup,
            profiles: self.profiles.as_slice(),
            workspaces: &self.workspaces,
        };
        Ok(toml::to_string_pretty(&serializable)?)
    }

    /// Find the action bound to an exact modifier/key combination.
    pub fn lookup(&self, modifiers: Modifiers, key: &str) -> Option<&Action> {
        self.bindings
            .iter()
            .find(|binding| {
                binding.key == key
                    && binding.modifiers.ctrl == modifiers.ctrl
                    && binding.modifiers.alt == modifiers.alt
                    && binding.modifiers.shift == modifiers.shift
                    && binding.modifiers.logo == modifiers.logo
            })
            .map(|binding| &binding.action)
    }
}

/// Raw, not yet validated configuration.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawConfig {
    layout: Option<LayoutConfig>,
    appearance: Option<Appearance>,
    theme: Option<ThemeConfig>,
    motion: Option<MotionConfig>,
    behavior: Option<Behavior>,
    rules: Option<Vec<WindowRule>>,
    bindings: Option<BTreeMap<String, String>>,
    startup: Option<Vec<String>>,
    profiles: Option<Vec<Profile>>,
    default_profile: Option<String>,
    workspaces: Option<Vec<WorkspacePreset>>,
}

/// Configuration failures.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read configuration at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid configuration: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("invalid configuration: {0}")]
    Invalid(String),
    #[error("invalid colour '{0}' (expected #rgb, #rrggbb or #rrggbbaa)")]
    InvalidColor(String),
    #[error("unknown modifier '{modifier}' in chord '{chord}'")]
    UnknownModifier { chord: String, modifier: String },
    #[error("duplicate keybinding for '{0}'")]
    DuplicateBinding(String),
    #[error(transparent)]
    Action(#[from] ActionParseError),
    #[error("could not serialize configuration: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// Parse `#rgb`, `#rrggbb` or `#rrggbbaa` into RGBA floats.
pub fn parse_hex_color(input: &str) -> Option<[f32; 4]> {
    let hex = input.trim().trim_start_matches('#');
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }

    let (r, g, b, a) = match hex.len() {
        3 => {
            let expand = |c: u8| c * 17;
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            (expand(r), expand(g), expand(b), 255)
        }
        6 => (
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
            255,
        ),
        8 => (
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
            u8::from_str_radix(&hex[6..8], 16).ok()?,
        ),
        _ => return None,
    };

    Some([
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_have_bindings() {
        let config = Config::default();
        config.layout.validate().expect("default layout is valid");
        assert!(config.bindings.len() >= 10);
        assert!(config
            .bindings
            .iter()
            .any(|binding| binding.key == "return" && binding.modifiers.logo));
    }

    #[test]
    fn empty_document_yields_defaults() {
        let config = Config::from_toml_str("").expect("empty config is valid");
        assert_eq!(config.bindings.len(), Config::default().bindings.len());
        assert_eq!(config.layout.gap, 8);
    }

    #[test]
    fn parses_layout_and_appearance() {
        let config = Config::from_toml_str(
            r##"
            [layout]
            gap = 4
            outer_margin = 0
            default_column_ratio = 0.6

            [appearance]
            background = "#1a1a22"
            "##,
        )
        .expect("valid config");

        assert_eq!(config.layout.gap, 4);
        assert_eq!(config.layout.outer_margin, 0);
        assert_eq!(config.layout.default_column_ratio, 0.6);
        let rgba = config.appearance.background_rgba();
        assert!((rgba[0] - 0.101_960_78).abs() < 1e-6);
        assert_eq!(rgba[3], 1.0);
    }

    #[test]
    fn absurd_gaps_and_margins_are_rejected() {
        // Such values overflowed the i32 geometry math (a panic in debug
        // builds, wrapped garbage rectangles in release builds).
        for text in [
            "[layout]\ngap = 2000000000\n",
            "[layout]\nouter_margin = 2000000000\n",
        ] {
            assert!(Config::from_toml_str(text).is_err(), "{text}");
        }
        Config::from_toml_str("[layout]\ngap = 64\nouter_margin = 64\n")
            .expect("ordinary values stay valid");
    }

    #[test]
    fn bindings_replace_defaults_when_present() {
        let config = Config::from_toml_str(
            r#"
            [bindings]
            "super+t" = "spawn foot"
            "super+shift+q" = "quit"
            "#,
        )
        .expect("valid config");

        assert_eq!(config.bindings.len(), 2);
        assert_eq!(
            config.lookup(
                Modifiers {
                    logo: true,
                    ..Default::default()
                },
                "t"
            ),
            Some(&Action::Spawn {
                command: "foot".into()
            })
        );
        assert_eq!(
            config.lookup(
                Modifiers {
                    logo: true,
                    shift: true,
                    ..Default::default()
                },
                "q"
            ),
            Some(&Action::Quit)
        );
        assert_eq!(
            config.lookup(
                Modifiers {
                    logo: true,
                    ..Default::default()
                },
                "q"
            ),
            None
        );
    }

    #[test]
    fn rejects_unknown_keys_and_bad_values() {
        assert!(matches!(
            Config::from_toml_str("[layout]\ngap = -1\n"),
            Err(ConfigError::Invalid(_))
        ));
        assert!(matches!(
            Config::from_toml_str("[layout]\nunknown = 3\n"),
            Err(ConfigError::Parse(_))
        ));
        assert!(matches!(
            Config::from_toml_str("[appearance]\nbackground = \"not-a-colour\"\n"),
            Err(ConfigError::InvalidColor(_))
        ));
        assert!(matches!(
            Config::from_toml_str("[bindings]\n\"hyper+x\" = \"close\"\n"),
            Err(ConfigError::UnknownModifier { .. })
        ));
        assert!(matches!(
            Config::from_toml_str("[bindings]\n\"super+x\" = \"explode\"\n"),
            Err(ConfigError::Action(_))
        ));
        assert!(matches!(
            Config::from_toml_str("[bindings]\n\"super+x\" = \"close\"\n\"super+x\" = \"quit\"\n"),
            Err(ConfigError::Parse(_))
        ));
    }

    #[test]
    fn duplicate_chords_are_rejected() {
        let error =
            Config::from_toml_str("[bindings]\n\"super+x\" = \"close\"\n\"super+X\" = \"quit\"\n");
        assert!(matches!(error, Err(ConfigError::DuplicateBinding(_))));
    }

    #[test]
    fn colour_parsing_handles_all_accepted_forms() {
        assert_eq!(parse_hex_color("#fff"), Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(parse_hex_color("000000"), Some([0.0, 0.0, 0.0, 1.0]));
        assert_eq!(
            parse_hex_color("#00ff0080"),
            Some([0.0, 1.0, 0.0, 128.0 / 255.0])
        );
        assert_eq!(parse_hex_color("#12345"), None);
        assert_eq!(parse_hex_color("#gggggg"), None);
    }

    #[test]
    fn parses_layout_mode_motion_and_rules() {
        let config = Config::from_toml_str(
            r##"
            [layout]
            mode = "auto"
            smart_gaps = true
            master_ratio = 0.65

            [motion]
            enabled = true
            duration_ms = 120
            reduce_motion = true

            [[rules]]
            app_id = "org.mozilla.firefox"
            title = "Picture-in-Picture"
            floating = true
            opacity = 0.85

            [[rules]]
            app_id = "signal"
            workspace = "chat"
            layout = "dwindle"
            "##,
        )
        .expect("valid config");

        assert_eq!(config.layout.mode, crate::tiling::LayoutKind::Auto);
        assert!(config.layout.smart_gaps);
        assert_eq!(config.layout.master_ratio, 0.65);
        assert!(config.motion.enabled);
        assert_eq!(config.motion.duration_ms, 120);
        assert!(config.motion.reduce_motion);
        assert_eq!(config.rules.rules().len(), 2);

        let policy = config
            .rules
            .resolve("org.mozilla.firefox", "Picture-in-Picture");
        assert!(policy.floating);
        assert_eq!(policy.opacity, 0.85);
    }

    #[test]
    fn invalid_motion_and_rules_are_rejected() {
        assert!(matches!(
            Config::from_toml_str("[motion]\nduration_ms = 60_000\n"),
            Err(ConfigError::Invalid(_))
        ));
        assert!(matches!(
            Config::from_toml_str("[[rules]]\nfloating = true\n"),
            Err(ConfigError::Invalid(_))
        ));
        assert!(matches!(
            Config::from_toml_str("[[rules]]\napp_id = \"foot\"\nopacity = 2.0\n"),
            Err(ConfigError::Invalid(_))
        ));
        assert!(matches!(
            Config::from_toml_str("[layout]\nmaster_ratio = 0.95\n"),
            Err(ConfigError::Invalid(_))
        ));
    }

    #[test]
    fn config_round_trips_through_toml() {
        let config = Config::default();
        let text = config.to_toml_string().expect("serializable");
        let reparsed = Config::from_toml_str(&text).expect("reparse");
        assert_eq!(reparsed.bindings.len(), config.bindings.len());
        assert_eq!(reparsed.layout.gap, config.layout.gap);
        assert_eq!(reparsed.workspaces, config.workspaces);
    }

    #[test]
    fn workspace_presets_are_parsed_and_validated() {
        let config = Config::from_toml_str(
            "[[workspaces]]\nname = \"code\"\nx = 2\ny = 0\nlayout = \"dwindle\"\n\n\
             [[workspaces]]\nname = \"chat\"\n",
        )
        .expect("valid config");
        assert_eq!(config.workspaces.len(), 2);
        assert_eq!(config.workspaces[0].position(), Some((2, 0)));
        assert_eq!(config.workspaces[0].layout, Some(LayoutKind::Dwindle));
        assert_eq!(config.workspaces[1].position(), None);

        assert!(Config::from_toml_str("[[workspaces]]\nname = \"x\"\nx = 1\n").is_err());
        assert!(Config::from_toml_str("[[workspaces]]\nname = \"  \"\n").is_err());
        assert!(Config::from_toml_str(
            "[[workspaces]]\nname = \"a\"\n\n[[workspaces]]\nname = \"a\"\n"
        )
        .is_err());
        assert!(Config::from_toml_str("[[workspaces]]\nname = \"a\"\ncolour = 1\n").is_err());
    }

    #[test]
    fn default_focus_chord_resolves_to_the_left_direction() {
        let config = Config::default();
        let action = config.lookup(
            Modifiers {
                logo: true,
                ..Default::default()
            },
            "left",
        );
        assert_eq!(
            action,
            Some(&Action::Focus {
                direction: crate::action::Direction::Left
            })
        );
    }
}
