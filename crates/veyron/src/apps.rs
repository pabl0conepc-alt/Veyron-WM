//! Installed-application discovery and launching for `veyronctl launch`.
//!
//! The XDG desktop-entry format is parsed by [`veyron_core::launcher`]; this
//! module owns the parts that touch the machine: which directories hold
//! applications, reading their files with bounds, and starting the process.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
};

use veyron_core::{best_match, parse_entry, DesktopEntry};

/// A single `.desktop` file larger than this is ignored instead of read.
const MAX_DESKTOP_FILE_BYTES: u64 = 64 * 1024;
/// Hard cap on applications held in memory for one lookup.
const MAX_APPLICATIONS: usize = 4096;
/// `applications/` directories nest one level in practice; keep a small bound.
const MAX_DIRECTORY_DEPTH: usize = 3;

/// Application directories in XDG precedence order.
fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match std::env::var_os("XDG_DATA_HOME") {
        Some(data_home) => dirs.push(PathBuf::from(data_home).join("applications")),
        None => {
            if let Some(home) = std::env::var_os("HOME") {
                dirs.push(PathBuf::from(home).join(".local/share/applications"));
            }
        }
    }
    match std::env::var_os("XDG_DATA_DIRS") {
        Some(data_dirs) => {
            dirs.extend(std::env::split_paths(&data_dirs).map(|dir| dir.join("applications")));
        }
        None => {
            dirs.push(PathBuf::from("/usr/local/share/applications"));
            dirs.push(PathBuf::from("/usr/share/applications"));
        }
    }
    dirs
}

/// Read every application under `dirs`. Earlier directories win when two files
/// share a name, as the XDG base directory specification requires.
fn entries_in(dirs: &[PathBuf]) -> Vec<DesktopEntry> {
    let mut found: BTreeMap<String, DesktopEntry> = BTreeMap::new();
    for dir in dirs {
        collect_dir(dir, 0, &mut found);
    }
    found.into_values().collect()
}

fn collect_dir(dir: &Path, depth: usize, found: &mut BTreeMap<String, DesktopEntry>) {
    if depth > MAX_DIRECTORY_DEPTH || found.len() >= MAX_APPLICATIONS {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_dir(&path, depth + 1, found);
            continue;
        }
        let Some(app_id) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".desktop"))
        else {
            continue;
        };
        if found.contains_key(app_id) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.len() > MAX_DESKTOP_FILE_BYTES {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(application) = parse_entry(app_id, &text) {
            found.insert(app_id.to_string(), application);
        }
    }
}

/// Resolve `query` to an installed application.
pub fn find(query: &str) -> Result<DesktopEntry, String> {
    let entries = entries_in(&data_dirs());
    best_match(&entries, query)
        .cloned()
        .ok_or_else(|| format!("no installed application matches '{query}'"))
}

/// Resolve and start `query`, returning the entry that was launched.
pub fn launch(query: &str) -> Result<DesktopEntry, String> {
    let entry = find(query)?;
    if entry.terminal {
        // Veyron does not assume a terminal emulator. A terminal application is
        // started by binding an explicit `spawn` action instead.
        return Err(format!(
            "'{}' needs a terminal, which the launcher does not start",
            entry.name
        ));
    }
    let (program, arguments) = entry.argv.split_first().ok_or("empty command line")?;
    let child = Command::new(program)
        .args(arguments)
        .spawn()
        .map_err(|error| format!("could not start '{}': {error}", entry.name))?;
    crate::state::reap(child);
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("veyron-apps-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn write_entry(dir: &Path, file: &str, name: &str, exec: &str) {
        std::fs::write(
            dir.join(file),
            format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\n"),
        )
        .expect("write entry");
    }

    #[test]
    fn earlier_directories_win_and_nested_entries_are_found() {
        let first = temp_dir("first");
        let second = temp_dir("second");
        write_entry(&first, "editor.desktop", "Editor A", "editor-a %U");
        write_entry(&second, "editor.desktop", "Editor B", "editor-b");
        let nested = second.join("nested");
        std::fs::create_dir_all(&nested).expect("nested dir");
        write_entry(&nested, "player.desktop", "Player", "player");

        let entries = entries_in(&[first.clone(), second.clone()]);
        assert_eq!(entries.len(), 2);
        assert_eq!(
            best_match(&entries, "editor").map(|entry| entry.name.as_str()),
            Some("Editor A"),
            "the first directory in XDG order wins"
        );
        assert!(best_match(&entries, "play").is_some());

        let _ = std::fs::remove_dir_all(&first);
        let _ = std::fs::remove_dir_all(&second);
    }

    #[test]
    fn unreadable_directories_are_skipped() {
        let missing = std::env::temp_dir().join(format!(
            "veyron-apps-missing-{}-{}",
            std::process::id(),
            "nope"
        ));
        let _ = std::fs::remove_dir_all(&missing);
        assert!(entries_in(&[missing]).is_empty());
    }
}
