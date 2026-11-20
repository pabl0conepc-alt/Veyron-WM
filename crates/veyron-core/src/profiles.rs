//! Named layout and motion presets — "Veyron Profiles".
//!
//! A profile is a small patch over the current layout and motion configuration,
//! so a user can keep several working styles (for example a dense tiling preset
//! and a spacious one) and switch between them from a keybinding or the control
//! socket. Only the fields a profile actually sets are touched, which means a
//! profile that changes the gap cannot silently reset the column ratios.
//!
//! Nothing here allocates per frame or knows about outputs: applying a profile
//! is a configuration change on a [`crate::wm::Wm`] and reuses the same
//! validation as startup configuration.

use serde::{Deserialize, Serialize};

use crate::{
    layout::LayoutConfig,
    motion::{MotionConfig, SpringConfig},
    tiling::LayoutKind,
};

/// Layout fields a profile may override. `None` keeps the current value.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LayoutPatch {
    pub gap: Option<i32>,
    pub outer_margin: Option<i32>,
    pub default_column_ratio: Option<f64>,
    pub min_column_ratio: Option<f64>,
    pub max_column_ratio: Option<f64>,
    /// Also selects this layout for every existing workspace.
    pub mode: Option<LayoutKind>,
    pub smart_gaps: Option<bool>,
    pub master_ratio: Option<f64>,
}

impl LayoutPatch {
    /// Copy every field the patch sets into `layout`.
    pub fn apply(&self, layout: &mut LayoutConfig) {
        if let Some(gap) = self.gap {
            layout.gap = gap;
        }
        if let Some(margin) = self.outer_margin {
            layout.outer_margin = margin;
        }
        if let Some(ratio) = self.default_column_ratio {
            layout.default_column_ratio = ratio;
        }
        if let Some(ratio) = self.min_column_ratio {
            layout.min_column_ratio = ratio;
        }
        if let Some(ratio) = self.max_column_ratio {
            layout.max_column_ratio = ratio;
        }
        if let Some(mode) = self.mode {
            layout.mode = mode;
        }
        if let Some(smart_gaps) = self.smart_gaps {
            layout.smart_gaps = smart_gaps;
        }
        if let Some(ratio) = self.master_ratio {
            layout.master_ratio = ratio;
        }
    }
}

/// Motion fields a profile may override. `None` keeps the current value.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MotionPatch {
    pub enabled: Option<bool>,
    pub duration_ms: Option<u32>,
    pub reduce_motion: Option<bool>,
    pub spring: Option<SpringConfig>,
}

impl MotionPatch {
    pub fn apply(&self, motion: &mut MotionConfig) {
        if let Some(enabled) = self.enabled {
            motion.enabled = enabled;
        }
        if let Some(duration) = self.duration_ms {
            motion.duration_ms = duration;
        }
        if let Some(reduce) = self.reduce_motion {
            motion.reduce_motion = reduce;
        }
        if let Some(spring) = self.spring {
            motion.spring = spring;
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == MotionPatch::default()
    }
}

/// One named preset.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub layout: LayoutPatch,
    pub motion: MotionPatch,
}

impl Profile {
    /// Validate the name and the patch as it would be applied to the default
    /// configuration. Names cannot contain whitespace because the action that
    /// selects a profile is parsed as `profile <name>`.
    pub fn validate(&self) -> Result<(), String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("profile name cannot be empty".to_string());
        }
        if name.len() > 64 {
            return Err(format!(
                "profile name '{}' is longer than 64 bytes",
                self.name
            ));
        }
        if name.chars().any(char::is_whitespace) {
            return Err(format!(
                "profile name '{}' cannot contain whitespace",
                self.name
            ));
        }

        let mut layout = LayoutConfig::default();
        self.layout.apply(&mut layout);
        layout
            .validate()
            .map_err(|error| format!("profile '{name}': {error}"))?;

        let mut motion = MotionConfig::default();
        self.motion.apply(&mut motion);
        motion
            .validate()
            .map_err(|error| format!("profile '{name}': {error}"))?;
        Ok(())
    }
}

/// The profiles a session knows about, in configuration order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Profiles {
    profiles: Vec<Profile>,
}

impl Profiles {
    /// Build a set, rejecting duplicate names and invalid patches.
    pub fn new(profiles: Vec<Profile>) -> Result<Self, String> {
        let mut seen: Vec<&str> = Vec::with_capacity(profiles.len());
        for profile in &profiles {
            profile.validate()?;
            if seen.contains(&profile.name.as_str()) {
                return Err(format!("duplicate profile '{}'", profile.name));
            }
            seen.push(&profile.name);
        }
        Ok(Self { profiles })
    }

    pub fn get(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|profile| profile.name == name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.profiles.iter().map(|profile| profile.name.as_str())
    }

    pub fn as_slice(&self) -> &[Profile] {
        &self.profiles
    }

    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }
}
