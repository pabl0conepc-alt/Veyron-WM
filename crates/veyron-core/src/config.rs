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
    /// Focus the window under the pointer without clicking. Windows only
    /// partly on screen still need a click, so focusing never scrolls the
    /// layout out from under the pointer.
    pub focus_follows_mouse: bool,
}

impl Default for Behavior {
    fn default() -> Self {
        Self {
            remember_geometry: true,
            smart_workspace: false,
            focus_follows_mouse: false,
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
    /// Files this configuration was read from: the main file first, then
    /// every `include` in load order. Empty when parsed from a string.
    pub sources: Vec<PathBuf>,
    /// Profile applied when the compositor starts.
    pub default_profile: Option<String>,
    /// Named workspaces created at startup on the primary output.
    pub workspaces: Vec<WorkspacePreset>,
}

/// Default chords. Kept in one place so the documentation, the default config
/// and the tests cannot disagree.
const DEFAULT_BINDINGS: &[(&str, &str)] = &[
    ("super+return", "terminal"),
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
            sources: Vec::new(),
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
        if raw.include.is_some() || raw.include_optional.is_some() {
            return Err(ConfigError::Invalid(
                "`include` works only in a configuration file, where paths have a base directory"
                    .to_string(),
            ));
        }
        Config::from_raw(raw)
    }

    fn from_raw(raw: RawConfig) -> Result<Config, ConfigError> {

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
            sources: Vec::new(),
        })
    }

    /// Load a configuration file and everything it includes.
    ///
    /// `include = ["keys.toml"]` and `include_optional = ["local.toml"]` name
    /// files relative to the including file (`~/` is the home directory).
    /// Included files are read first, in order, and the including file is
    /// merged on top: tables merge key by key (later wins), arrays of tables
    /// such as `[[rules]]` are appended. A missing optional file is skipped.
    pub fn load(path: &std::path::Path) -> Result<Config, ConfigError> {
        let mut sources = Vec::new();
        let table = load_table(path, &mut Vec::new(), &mut sources)?;
        let raw: RawConfig = table.try_into()?;
        let mut config = Config::from_raw(raw)?;
        config.sources = sources;
        Ok(config)
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
    /// Files merged under this one; resolved by [`Config::load`].
    include: Option<Vec<String>>,
    include_optional: Option<Vec<String>>,
}

/// How deep `include` may nest before it is treated as a mistake.
const MAX_INCLUDE_DEPTH: usize = 8;

/// Read `path` and the files it includes into one merged TOML table.
/// `stack` holds the files being read, to report include cycles; every file
/// read is appended to `sources` so callers can watch them all.
fn load_table(
    path: &std::path::Path,
    stack: &mut Vec<PathBuf>,
    sources: &mut Vec<PathBuf>,
) -> Result<toml::Table, ConfigError> {
    if stack.len() > MAX_INCLUDE_DEPTH {
        return Err(ConfigError::Invalid(format!(
            "includes nest deeper than {MAX_INCLUDE_DEPTH} files at {}",
            path.display()
        )));
    }
    let identity = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if stack.contains(&identity) {
        return Err(ConfigError::Invalid(format!(
            "{} includes itself",
            path.display()
        )));
    }
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut table: toml::Table = toml::from_str(&text).map_err(|error| ConfigError::InFile {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    sources.push(path.to_path_buf());

    let base = path.parent().unwrap_or(std::path::Path::new("."));
    let mut merged = toml::Table::new();
    stack.push(identity);
    for (key, optional) in [("include", false), ("include_optional", true)] {
        let Some(value) = table.remove(key) else {
            continue;
        };
        let entries: Vec<String> = value.try_into().map_err(|_| {
            ConfigError::Invalid(format!(
                "`{key}` in {} must be a list of file names",
                path.display()
            ))
        })?;
        for entry in entries {
            let included = resolve_include(base, &entry);
            match load_table(&included, stack, sources) {
                Ok(table) => merge_tables(&mut merged, table),
                Err(ConfigError::Io { source, .. })
                    if optional && source.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    stack.pop();
                    return Err(error);
                }
            }
        }
    }
    stack.pop();
    merge_tables(&mut merged, table);
    Ok(merged)
}

/// An include path: `~/` is the home directory, relative paths start at the
/// including file's directory.
fn resolve_include(base: &std::path::Path, entry: &str) -> PathBuf {
    if let Some(rest) = entry.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME").filter(|home| !home.is_empty()) {
            return PathBuf::from(home).join(rest);
        }
    }
    base.join(entry)
}

/// Merge `top` into `base`: tables merge recursively, arrays of tables (such
/// as `[[rules]]`) are appended, anything else in `top` replaces `base`.
fn merge_tables(base: &mut toml::Table, top: toml::Table) {
    for (key, value) in top {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(existing)), toml::Value::Table(table)) => {
                merge_tables(existing, table);
            }
            (Some(toml::Value::Array(existing)), toml::Value::Array(array))
                if array.iter().all(toml::Value::is_table)
                    && existing.iter().all(toml::Value::is_table) =>
            {
                existing.extend(array);
            }
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
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
    #[error("invalid configuration in {path}: {message}")]
    InFile { path: PathBuf, message: String },
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
    fn focus_follows_mouse_is_off_by_default_and_can_be_enabled() {
        assert!(!Config::default().behavior.focus_follows_mouse);
        let config = Config::from_toml_str("[behavior]\nfocus_follows_mouse = true\n")
            .expect("valid configuration");
        assert!(config.behavior.focus_follows_mouse);
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

    /// A fresh directory for include tests.
    fn include_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "veyron-include-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create test directory");
        dir
    }

    #[test]
    fn includes_merge_tables_append_rules_and_let_the_includer_win() {
        let dir = include_dir("merge");
        std::fs::create_dir_all(dir.join("parts")).expect("parts directory");
        std::fs::write(
            dir.join("parts/look.toml"),
            "[layout]\ngap = 20\nouter_margin = 4\n\n[[rules]]\napp_id = \"mpv\"\nfloating = true\n",
        )
        .expect("write include");
        std::fs::write(
            dir.join("veyron.toml"),
            "include = [\"parts/look.toml\"]\ninclude_optional = [\"missing.toml\"]\n\n\
             [layout]\ngap = 6\n\n[[rules]]\napp_id = \"signal\"\nworkspace = \"chat\"\n",
        )
        .expect("write main file");

        let config = Config::load(&dir.join("veyron.toml")).expect("loads with includes");
        assert_eq!(config.layout.gap, 6, "the including file wins");
        assert_eq!(config.layout.outer_margin, 4, "untouched keys come from the include");
        assert_eq!(config.rules.rules().len(), 2, "rules from both files are kept");
        assert!(config.rules.resolve("mpv", "").floating);
        assert_eq!(
            config.sources,
            vec![dir.join("veyron.toml"), dir.join("parts/look.toml")]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_required_includes_and_cycles_are_errors() {
        let dir = include_dir("errors");
        std::fs::write(dir.join("a.toml"), "include = [\"gone.toml\"]\n").expect("write");
        assert!(matches!(
            Config::load(&dir.join("a.toml")),
            Err(ConfigError::Io { .. })
        ));

        std::fs::write(dir.join("b.toml"), "include = [\"c.toml\"]\n").expect("write");
        std::fs::write(dir.join("c.toml"), "include = [\"b.toml\"]\n").expect("write");
        match Config::load(&dir.join("b.toml")) {
            Err(ConfigError::Invalid(message)) => assert!(message.contains("includes itself")),
            other => panic!("a cycle must be reported: {other:?}"),
        }

        std::fs::write(dir.join("d.toml"), "include = [\"e.toml\"]\n").expect("write");
        std::fs::write(dir.join("e.toml"), "[layout\n").expect("write");
        match Config::load(&dir.join("d.toml")) {
            Err(ConfigError::InFile { path, .. }) => assert_eq!(path, dir.join("e.toml")),
            other => panic!("a broken include must name its file: {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn includes_need_a_file_to_be_relative_to() {
        assert!(Config::from_toml_str("include = [\"keys.toml\"]\n").is_err());
    }
}
