//! Veyron Theme: Material You colour schemes for the compositor and the
//! desktop around it.
//!
//! A scheme is generated from one seed colour with the Material Color
//! Utilities port (`material-colors`), the same generator Material You and
//! Caelestia-style shells use. The compositor draws window borders from it,
//! and [`Palette`] renders the scheme into files that bars, GTK apps and shell
//! widgets read (`colors.json`, `colors.css`, `colors.scss`, `waybar.css`), so
//! the whole desktop is restyled by editing `[theme]` or sending a `theme`
//! action, without writing shell scripts.

use std::{collections::BTreeMap, fmt::Write as _};

use material_colors::{
    color::Rgb,
    dynamic_color::{DynamicScheme, Variant},
};
use serde::{Deserialize, Serialize};

use crate::config::parse_hex_color;

/// Named seeds usable instead of a hex colour, so a scheme can be picked by
/// name from a binding or `veyronctl` without remembering colours.
pub const PRESETS: &[(&str, &str)] = &[
    ("material", "#6750a4"),
    ("ocean", "#0061a4"),
    ("forest", "#386a20"),
    ("ember", "#9c4146"),
    ("amber", "#7c5800"),
    ("teal", "#006a6a"),
    ("rose", "#984061"),
    ("slate", "#535f70"),
];

/// Scheme variants, in the order `theme variant next` cycles through them.
pub const VARIANTS: &[&str] = &[
    "tonal-spot",
    "vibrant",
    "expressive",
    "fidelity",
    "content",
    "neutral",
    "monochrome",
    "rainbow",
    "fruit-salad",
];

/// `[theme]` configuration table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeConfig {
    /// Seed colour (`#rrggbb`) or a preset name from [`PRESETS`].
    pub seed: String,
    /// `dark` or `light`.
    pub mode: ThemeMode,
    /// Material scheme variant, one of [`VARIANTS`].
    pub variant: String,
    /// Contrast level from -1.0 (reduced) to 1.0 (high); 0.0 is standard.
    pub contrast: f64,
    /// Window decoration style, one of [`DESIGNS`]. The fields below
    /// override single values of the chosen style.
    pub design: String,
    /// Border drawn around windows, in logical pixels. 0 disables borders.
    pub border_width: Option<i32>,
    /// Radius of rounded window corners, in logical pixels. 0 keeps corners
    /// square.
    pub corner_radius: Option<i32>,
    /// Blur radius of the drop shadow, in logical pixels. 0 disables shadows.
    pub shadow_size: Option<i32>,
    /// Opacity of the drop shadow at the window edge (`0.0..=1.0`).
    pub shadow_opacity: Option<f32>,
    /// Opacity applied to unfocused windows (`0.1..=1.0`), on top of rules.
    pub inactive_opacity: Option<f32>,
    /// Overrides the focused-window border colour (defaults to `primary`).
    pub border_active: Option<String>,
    /// Overrides the unfocused-window border colour (defaults to
    /// `outline_variant`).
    pub border_inactive: Option<String>,
    /// Clear the desktop with the scheme's `surface` colour instead of
    /// `appearance.background`.
    pub use_surface_background: bool,
    /// Write the scheme to `$XDG_STATE_HOME/veyron/theme/` whenever it changes.
    pub export: bool,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            seed: "material".to_string(),
            mode: ThemeMode::Dark,
            variant: "tonal-spot".to_string(),
            contrast: 0.0,
            design: "material".to_string(),
            border_width: None,
            corner_radius: None,
            shadow_size: None,
            shadow_opacity: None,
            inactive_opacity: None,
            border_active: None,
            border_inactive: None,
            use_surface_background: false,
            export: true,
        }
    }
}

/// Light or dark scheme.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
}

impl ThemeMode {
    pub fn toggled(self) -> Self {
        match self {
            ThemeMode::Dark => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::Dark,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ThemeMode::Dark => "dark",
            ThemeMode::Light => "light",
        }
    }
}

impl ThemeConfig {
    pub fn validate(&self) -> Result<(), String> {
        resolve_seed(&self.seed)?;
        parse_variant(&self.variant)?;
        if !(-1.0..=1.0).contains(&self.contrast) {
            return Err(format!(
                "theme.contrast must be between -1.0 and 1.0, got {}",
                self.contrast
            ));
        }
        Design::preset(&self.design)?;
        let ranges = [
            ("border_width", self.border_width, 0..=32),
            ("corner_radius", self.corner_radius, 0..=64),
            ("shadow_size", self.shadow_size, 0..=96),
        ];
        for (name, value, range) in ranges {
            if let Some(value) = value.filter(|value| !range.contains(value)) {
                return Err(format!(
                    "theme.{name} must be between {} and {}, got {value}",
                    range.start(),
                    range.end()
                ));
            }
        }
        if let Some(opacity) = self.shadow_opacity.filter(|o| !(0.0..=1.0).contains(o)) {
            return Err(format!(
                "theme.shadow_opacity must be between 0.0 and 1.0, got {opacity}"
            ));
        }
        if let Some(opacity) = self.inactive_opacity.filter(|o| !(0.1..=1.0).contains(o)) {
            return Err(format!(
                "theme.inactive_opacity must be between 0.1 and 1.0, got {opacity}"
            ));
        }
        for colour in [&self.border_active, &self.border_inactive]
            .into_iter()
            .flatten()
        {
            if parse_hex_color(colour).is_none() {
                return Err(format!("invalid theme border colour '{colour}'"));
            }
        }
        Ok(())
    }

    /// Generate the palette. Invalid fields fall back to defaults, which only
    /// happens for a config that bypassed [`ThemeConfig::validate`].
    pub fn palette(&self) -> Palette {
        let seed = resolve_seed(&self.seed).unwrap_or(Rgb::from_u32(0x6750a4));
        let variant = parse_variant(&self.variant).unwrap_or(Variant::TonalSpot);
        let scheme = DynamicScheme::by_variant(
            seed,
            &variant,
            self.mode == ThemeMode::Dark,
            Some(self.contrast),
        );
        Palette::from_scheme(&scheme, self)
    }

    /// Switch to another decoration style at runtime. Per-field overrides are
    /// dropped so the chosen style is shown as designed.
    pub fn set_design(&mut self, design: &str) {
        self.design = design.to_ascii_lowercase();
        self.border_width = None;
        self.corner_radius = None;
        self.shadow_size = None;
        self.shadow_opacity = None;
        self.inactive_opacity = None;
    }

    /// The decoration style with this table's overrides applied. Invalid
    /// names fall back to `material`.
    pub fn design(&self) -> Design {
        let mut design = Design::preset(&self.design).unwrap_or_else(|_| Design::material());
        if let Some(width) = self.border_width {
            design.border_width = width;
        }
        if let Some(radius) = self.corner_radius {
            design.corner_radius = radius;
        }
        if let Some(size) = self.shadow_size {
            design.shadow_size = size;
        }
        if let Some(opacity) = self.shadow_opacity {
            design.shadow_opacity = opacity;
        }
        if let Some(opacity) = self.inactive_opacity {
            design.inactive_opacity = opacity;
        }
        design
    }
}

/// Decoration styles selectable with `design = "<name>"` or `theme design`.
pub const DESIGNS: &[&str] = &["material", "glass", "flat", "neon", "minimal"];

/// How windows are decorated: borders, corners, shadow and inactive dimming.
/// Colours always come from the Material palette.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Design {
    pub border_width: i32,
    pub corner_radius: i32,
    pub shadow_size: i32,
    pub shadow_opacity: f32,
    /// Vertical shadow offset, so light seems to come from above.
    pub shadow_offset: i32,
    pub inactive_opacity: f32,
    /// Tint the focused window's shadow with its border colour (a glow).
    pub glow: bool,
}

impl Design {
    /// Material 3: medium rounding and a soft elevation shadow.
    pub fn material() -> Self {
        Self {
            border_width: 2,
            corner_radius: 12,
            shadow_size: 18,
            shadow_opacity: 0.35,
            shadow_offset: 4,
            inactive_opacity: 1.0,
            glow: false,
        }
    }

    pub fn preset(name: &str) -> Result<Self, String> {
        let material = Self::material();
        Ok(match name.trim().to_ascii_lowercase().as_str() {
            "material" => material,
            // Frosted look: large radius, hairline border, dimmed background
            // windows so the focused one stands out.
            "glass" => Self {
                border_width: 1,
                corner_radius: 16,
                shadow_size: 24,
                shadow_opacity: 0.25,
                inactive_opacity: 0.88,
                ..material
            },
            // Square, no shadow: cheapest to draw.
            "flat" => Self {
                border_width: 1,
                corner_radius: 0,
                shadow_size: 0,
                shadow_opacity: 0.0,
                shadow_offset: 0,
                ..material
            },
            // Thick border with a coloured glow around the focused window.
            "neon" => Self {
                border_width: 3,
                corner_radius: 8,
                shadow_size: 16,
                shadow_opacity: 0.6,
                shadow_offset: 0,
                glow: true,
                ..material
            },
            // No border, only a faint shadow and slight rounding.
            "minimal" => Self {
                border_width: 0,
                corner_radius: 6,
                shadow_size: 10,
                shadow_opacity: 0.2,
                shadow_offset: 2,
                ..material
            },
            _ => {
                return Err(format!(
                    "unknown design '{name}' (expected one of {})",
                    DESIGNS.join(", ")
                ))
            }
        })
    }

    /// Whether any GPU decoration (rounding, shadow) is needed at all.
    pub fn needs_shader(&self) -> bool {
        self.corner_radius > 0 || (self.shadow_size > 0 && self.shadow_opacity > 0.0)
    }
}

/// Resolve a preset name or `#rrggbb` into a colour.
pub fn resolve_seed(seed: &str) -> Result<Rgb, String> {
    let text = PRESETS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(seed.trim()))
        .map_or(seed.trim(), |(_, hex)| hex);
    let digits = text.strip_prefix('#').unwrap_or(text);
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "theme seed '{seed}' is neither #rrggbb nor a preset ({})",
            PRESETS
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    u32::from_str_radix(digits, 16)
        .map(Rgb::from_u32)
        .map_err(|error| error.to_string())
}

pub fn parse_variant(name: &str) -> Result<Variant, String> {
    Ok(
        match name.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "tonal-spot" | "tonalspot" => Variant::TonalSpot,
            "vibrant" => Variant::Vibrant,
            "expressive" => Variant::Expressive,
            "fidelity" => Variant::Fidelity,
            "content" => Variant::Content,
            "neutral" => Variant::Neutral,
            "monochrome" => Variant::Monochrome,
            "rainbow" => Variant::Rainbow,
            "fruit-salad" | "fruitsalad" => Variant::FruitSalad,
            _ => {
                return Err(format!(
                    "unknown theme variant '{name}' (expected one of {})",
                    VARIANTS.join(", ")
                ))
            }
        },
    )
}

/// A generated scheme: Material role name (snake case) to `#rrggbb`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub mode: ThemeMode,
    pub roles: BTreeMap<&'static str, String>,
    /// Focused and unfocused border colours (`#rrggbbaa` allowed).
    pub border_active: String,
    pub border_inactive: String,
}

macro_rules! roles {
    ($scheme:expr, $($role:ident),* $(,)?) => {{
        let mut roles = BTreeMap::new();
        $( roles.insert(stringify!($role), hex($scheme.$role())); )*
        roles
    }};
}

fn hex(rgb: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb.red, rgb.green, rgb.blue)
}

impl Palette {
    fn from_scheme(scheme: &DynamicScheme, config: &ThemeConfig) -> Self {
        let roles = roles!(
            scheme,
            background,
            on_background,
            surface,
            surface_dim,
            surface_bright,
            surface_container_lowest,
            surface_container_low,
            surface_container,
            surface_container_high,
            surface_container_highest,
            on_surface,
            surface_variant,
            on_surface_variant,
            inverse_surface,
            inverse_on_surface,
            outline,
            outline_variant,
            shadow,
            scrim,
            surface_tint,
            primary,
            on_primary,
            primary_container,
            on_primary_container,
            inverse_primary,
            secondary,
            on_secondary,
            secondary_container,
            on_secondary_container,
            tertiary,
            on_tertiary,
            tertiary_container,
            on_tertiary_container,
            error,
            on_error,
            error_container,
            on_error_container,
        );
        let border_active = config
            .border_active
            .clone()
            .unwrap_or_else(|| roles["primary"].clone());
        let border_inactive = config
            .border_inactive
            .clone()
            .unwrap_or_else(|| roles["outline_variant"].clone());
        Self {
            mode: config.mode,
            roles,
            border_active,
            border_inactive,
        }
    }

    /// RGBA floats of a role, for the renderer.
    pub fn rgba(&self, role: &str) -> Option<[f32; 4]> {
        self.roles.get(role).and_then(|hex| parse_hex_color(hex))
    }

    pub fn border_active_rgba(&self) -> [f32; 4] {
        parse_hex_color(&self.border_active).unwrap_or([1.0, 1.0, 1.0, 1.0])
    }

    pub fn border_inactive_rgba(&self) -> [f32; 4] {
        parse_hex_color(&self.border_inactive).unwrap_or([0.3, 0.3, 0.3, 1.0])
    }

    /// `{"mode": "dark", "colors": {"primary": "#…", …}}`.
    pub fn to_json(&self) -> String {
        let mut out = format!(
            "{{\n  \"mode\": \"{}\",\n  \"colors\": {{",
            self.mode.as_str()
        );
        for (index, (role, hex)) in self.roles.iter().enumerate() {
            let comma = if index + 1 == self.roles.len() {
                ""
            } else {
                ","
            };
            let _ = write!(out, "\n    \"{role}\": \"{hex}\"{comma}");
        }
        out.push_str("\n  }\n}\n");
        out
    }

    /// GTK 3/4 named colours (`@define-color primary #…;`), also understood
    /// by waybar, swaync and other GTK-CSS based tools.
    pub fn to_gtk_css(&self) -> String {
        let mut out = String::from("/* Generated by Veyron WM. Edit [theme] instead. */\n");
        for (role, hex) in &self.roles {
            let _ = writeln!(out, "@define-color {role} {hex};");
        }
        out
    }

    /// Sass variables (`$primary: #…;`) for eww and other SCSS-based widgets.
    pub fn to_scss(&self) -> String {
        let mut out = String::from("// Generated by Veyron WM. Edit [theme] instead.\n");
        for (role, hex) in &self.roles {
            let _ = writeln!(out, "${}: {hex};", role.replace('_', "-"));
        }
        out
    }

    /// A complete Material-styled waybar stylesheet built on the GTK colours.
    /// Users can `@import` it, or keep their own and import `colors.css`.
    pub fn to_waybar_css(&self) -> String {
        let mut out = self.to_gtk_css();
        out.push_str(WAYBAR_STYLE);
        out
    }

    /// Every exported file as `(file name, contents)`.
    pub fn exports(&self) -> Vec<(&'static str, String)> {
        vec![
            ("colors.json", self.to_json()),
            ("colors.css", self.to_gtk_css()),
            ("colors.scss", self.to_scss()),
            ("waybar.css", self.to_waybar_css()),
        ]
    }
}

/// Material 3 bar: rounded pill modules on a translucent surface, the active
/// workspace filled with the primary container.
const WAYBAR_STYLE: &str = r#"
* {
    font-family: "Inter", "Roboto", "Noto Sans", sans-serif;
    font-size: 13px;
    min-height: 0;
    border: none;
}

window#waybar {
    background: alpha(@surface, 0.85);
    color: @on_surface;
}

#workspaces, #clock, #battery, #pulseaudio, #network, #cpu, #memory,
#backlight, #tray, #custom-power, #window, #mpris {
    background: @surface_container_high;
    color: @on_surface;
    border-radius: 16px;
    padding: 2px 12px;
    margin: 4px 3px;
}

#workspaces button {
    color: @on_surface_variant;
    border-radius: 12px;
    padding: 0 8px;
}

#workspaces button.active, #workspaces button.focused {
    background: @primary_container;
    color: @on_primary_container;
}

#workspaces button.urgent, #battery.critical {
    background: @error_container;
    color: @on_error_container;
}

#clock {
    background: @primary;
    color: @on_primary;
    font-weight: bold;
}

tooltip {
    background: @surface_container;
    color: @on_surface;
    border-radius: 12px;
    border: 1px solid @outline_variant;
}
"#;

/// Directory the exported theme files are written to:
/// `$XDG_STATE_HOME/veyron/theme`, or `~/.local/state/veyron/theme`.
pub fn export_dir() -> Option<std::path::PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()) {
        return Some(std::path::PathBuf::from(dir).join("veyron").join("theme"));
    }
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(|home| {
            std::path::PathBuf::from(home)
                .join(".local")
                .join("state")
                .join("veyron")
                .join("theme")
        })
}

/// Write the palette's files into `dir`, rewriting only files whose contents
/// changed so file watchers (waybar `reload_style_on_change`, eww) do not
/// reload needlessly. Returns how many files were written.
pub fn export(palette: &Palette, dir: &std::path::Path) -> std::io::Result<usize> {
    std::fs::create_dir_all(dir)?;
    let mut written = 0;
    for (name, contents) in palette.exports() {
        let path = dir.join(name);
        if std::fs::read_to_string(&path).ok().as_deref() == Some(contents.as_str()) {
            continue;
        }
        // Write then rename so readers never observe a half-written file.
        let temporary = dir.join(format!(".{name}.tmp"));
        std::fs::write(&temporary, contents)?;
        std::fs::rename(&temporary, &path)?;
        written += 1;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_is_valid_and_dark() {
        let config = ThemeConfig::default();
        config.validate().expect("default theme validates");
        let palette = config.palette();
        assert_eq!(palette.mode, ThemeMode::Dark);
        assert_eq!(palette.border_active, palette.roles["primary"]);
        // Dark schemes put light text on dark surfaces.
        let surface = palette.rgba("surface").expect("surface");
        let on_surface = palette.rgba("on_surface").expect("on_surface");
        assert!(
            surface[0] + surface[1] + surface[2] < on_surface[0] + on_surface[1] + on_surface[2]
        );
    }

    #[test]
    fn light_mode_inverts_surface_luminance() {
        let config = ThemeConfig {
            mode: ThemeMode::Light,
            ..ThemeConfig::default()
        };
        let palette = config.palette();
        let surface = palette.rgba("surface").expect("surface");
        let on_surface = palette.rgba("on_surface").expect("on_surface");
        assert!(
            surface[0] + surface[1] + surface[2] > on_surface[0] + on_surface[1] + on_surface[2]
        );
    }

    #[test]
    fn presets_hex_and_variants_resolve() {
        for (name, hex) in PRESETS {
            assert_eq!(resolve_seed(name), resolve_seed(hex));
        }
        assert!(resolve_seed("#12345").is_err());
        assert!(resolve_seed("nope").is_err());
        for variant in VARIANTS {
            parse_variant(variant).expect("listed variant parses");
        }
        assert!(parse_variant("plaid").is_err());
    }

    #[test]
    fn different_seeds_give_different_primaries() {
        let a = ThemeConfig::default().palette();
        let b = ThemeConfig {
            seed: "forest".into(),
            ..ThemeConfig::default()
        }
        .palette();
        assert_ne!(a.roles["primary"], b.roles["primary"]);
    }

    #[test]
    fn every_design_resolves_and_overrides_win() {
        for name in DESIGNS {
            let config = ThemeConfig {
                design: (*name).into(),
                ..ThemeConfig::default()
            };
            config.validate().expect("listed design validates");
            assert_eq!(config.design(), Design::preset(name).expect("preset"));
        }
        assert!(!Design::preset("flat").expect("flat").needs_shader());
        assert!(Design::preset("material").expect("material").needs_shader());

        let config = ThemeConfig {
            design: "flat".into(),
            corner_radius: Some(20),
            inactive_opacity: Some(0.5),
            ..ThemeConfig::default()
        };
        let design = config.design();
        assert_eq!(design.corner_radius, 20);
        assert_eq!(design.inactive_opacity, 0.5);
        assert_eq!(design.shadow_size, 0, "untouched fields keep the preset");

        let bad = ThemeConfig {
            design: "baroque".into(),
            ..ThemeConfig::default()
        };
        assert!(bad.validate().is_err());
        for bad in [
            ThemeConfig {
                corner_radius: Some(65),
                ..ThemeConfig::default()
            },
            ThemeConfig {
                shadow_opacity: Some(1.5),
                ..ThemeConfig::default()
            },
            ThemeConfig {
                inactive_opacity: Some(0.0),
                ..ThemeConfig::default()
            },
        ] {
            assert!(bad.validate().is_err());
        }
    }

    #[test]
    fn validation_rejects_out_of_range_values() {
        for config in [
            ThemeConfig {
                contrast: 2.0,
                ..ThemeConfig::default()
            },
            ThemeConfig {
                border_width: Some(-1),
                ..ThemeConfig::default()
            },
            ThemeConfig {
                border_active: Some("red".into()),
                ..ThemeConfig::default()
            },
        ] {
            assert!(config.validate().is_err());
        }
    }

    #[test]
    fn exports_cover_every_role_and_skip_unchanged_files() {
        let palette = ThemeConfig::default().palette();
        let css = palette.to_gtk_css();
        let scss = palette.to_scss();
        for (role, hex) in &palette.roles {
            assert!(css.contains(&format!("@define-color {role} {hex};")));
            assert!(scss.contains(&format!("${}: {hex};", role.replace('_', "-"))));
        }
        let json = palette.to_json();
        assert!(json.contains("\"mode\": \"dark\""));
        assert!(palette.to_waybar_css().contains("window#waybar"));

        let dir = std::env::temp_dir().join(format!("veyron-theme-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(export(&palette, &dir).expect("first export"), 4);
        assert_eq!(export(&palette, &dir).expect("second export"), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
