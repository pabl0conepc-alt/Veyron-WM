//! XDG desktop-entry parsing for the Veyron application launcher.
//!
//! The parser is pure: it turns the text of a `.desktop` file into a
//! [`DesktopEntry`] and answers "which installed application does this query
//! refer to". The compositor owns the filesystem scan and the process spawn, so
//! matching rules stay unit-testable without reading the user's real
//! `~/.local/share/applications`.
//!
//! Only the `[Desktop Entry]` group is read. Field codes in `Exec` are removed:
//! Veyron starts one application without files or URLs, so `%f`, `%U` and
//! friends have nothing to expand to.

/// One application advertised by a `.desktop` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopEntry {
    /// Identifier, normally the file name without the `.desktop` suffix.
    pub app_id: String,
    /// Display name from the entry.
    pub name: String,
    /// Command line with field codes removed.
    pub argv: Vec<String>,
    /// Whether the application asks for a terminal.
    pub terminal: bool,
}

impl DesktopEntry {
    /// Command line as one printable string.
    pub fn command(&self) -> String {
        self.argv.join(" ")
    }
}

/// Parse one `.desktop` file. `app_id` is the file name without the suffix.
///
/// Returns `None` for entries that are not applications, that are marked
/// `Hidden`/`NoDisplay`, or that have no usable `Name`/`Exec`.
pub fn parse_entry(app_id: &str, text: &str) -> Option<DesktopEntry> {
    let mut in_group = false;
    let mut kind = String::new();
    let mut name = String::new();
    let mut exec = String::new();
    let mut terminal = false;
    let mut hidden = false;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_group = line == "[Desktop Entry]";
            continue;
        }
        if !in_group {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.contains('[') {
            // Localised value such as `Name[pt_BR]`; the plain key wins.
            continue;
        }
        let value = value.trim();
        match key {
            "Type" => kind = value.to_string(),
            "Name" => name = value.to_string(),
            "Exec" => exec = value.to_string(),
            "Terminal" => terminal = value.eq_ignore_ascii_case("true"),
            "Hidden" | "NoDisplay" => hidden |= value.eq_ignore_ascii_case("true"),
            _ => {}
        }
    }

    if hidden || !(kind.is_empty() || kind == "Application") {
        return None;
    }
    if name.is_empty() || exec.is_empty() {
        return None;
    }
    let argv = split_exec(&exec);
    if argv.is_empty() {
        return None;
    }
    Some(DesktopEntry {
        app_id: app_id.to_string(),
        name,
        argv,
        terminal,
    })
}

/// Split an `Exec` line into arguments and drop desktop-entry field codes.
///
/// Whitespace separates arguments, double quotes group them and a backslash
/// escapes the next character, as the Desktop Entry specification requires.
pub fn split_exec(exec: &str) -> Vec<String> {
    let mut argv = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut has_token = false;
    let mut chars = exec.chars().peekable();

    while let Some(character) = chars.next() {
        match character {
            '\\' => {
                if let Some(escaped) = chars.next() {
                    current.push(escaped);
                    has_token = true;
                }
            }
            '"' => {
                quoted = !quoted;
                has_token = true;
            }
            c if c.is_whitespace() && !quoted => {
                if has_token {
                    argv.push(std::mem::take(&mut current));
                    has_token = false;
                }
            }
            c => {
                current.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        argv.push(current);
    }

    argv.into_iter()
        .filter_map(|argument| {
            let cleaned = remove_field_codes(&argument);
            if cleaned.is_empty() {
                None
            } else {
                Some(cleaned)
            }
        })
        .collect()
}

/// Remove `%f`, `%U`, `%c` and friends; `%%` becomes a literal `%`.
fn remove_field_codes(argument: &str) -> String {
    let mut cleaned = String::with_capacity(argument.len());
    let mut chars = argument.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '%' {
            cleaned.push(character);
            continue;
        }
        match chars.next() {
            Some('%') => cleaned.push('%'),
            // File, URL, icon, name and class codes have nothing to expand to.
            Some('f' | 'F' | 'u' | 'U' | 'i' | 'c' | 'k') | None => {}
            Some(other) => {
                cleaned.push('%');
                cleaned.push(other);
            }
        }
    }
    cleaned
}

/// Pick the application a query refers to.
///
/// Ranking: an exact case-insensitive name or app id, then a prefix match, then
/// a substring match. Ties break alphabetically so the choice is stable.
pub fn best_match<'a>(entries: &'a [DesktopEntry], query: &str) -> Option<&'a DesktopEntry> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return None;
    }

    let mut candidates: Vec<(u8, &DesktopEntry)> = entries
        .iter()
        .filter_map(|entry| {
            let name = entry.name.to_lowercase();
            let app_id = entry.app_id.to_lowercase();
            let rank = if name == query || app_id == query {
                0
            } else if name.starts_with(&query) || app_id.starts_with(&query) {
                1
            } else if name.contains(&query) || app_id.contains(&query) {
                2
            } else {
                return None;
            };
            Some((rank, entry))
        })
        .collect();

    candidates.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.name.to_lowercase().cmp(&right.1.name.to_lowercase()))
    });
    candidates.first().map(|(_, entry)| *entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EDITOR: &str = "\
[Desktop Entry]
Type=Application
Name=Text Editor
Exec=editor --new-window %U
Icon=editor
";

    #[test]
    fn parses_an_application_entry() {
        let entry = parse_entry("editor", EDITOR).expect("application");
        assert_eq!(entry.app_id, "editor");
        assert_eq!(entry.name, "Text Editor");
        assert_eq!(entry.argv, ["editor", "--new-window"]);
        assert_eq!(entry.command(), "editor --new-window");
        assert!(!entry.terminal);
    }

    #[test]
    fn rejects_hidden_localised_and_non_application_entries() {
        assert!(parse_entry(
            "hidden",
            "[Desktop Entry]\nType=Application\nName=Hidden\nExec=x\nHidden=true\n"
        )
        .is_none());
        assert!(parse_entry(
            "nodisplay",
            "[Desktop Entry]\nType=Application\nName=No\nExec=x\nNoDisplay=true\n"
        )
        .is_none());
        assert!(parse_entry("link", "[Desktop Entry]\nType=Link\nName=Link\nExec=x\n").is_none());
        assert!(parse_entry("noexec", "[Desktop Entry]\nType=Application\nName=Foo\n").is_none());

        let localised = parse_entry(
            "term",
            "[Desktop Entry]\nType=Application\nName[pt_BR]=Terminal\nName=Terminal\nExec=term\nTerminal=true\n",
        )
        .expect("localised keys are skipped");
        assert_eq!(localised.name, "Terminal");
        assert!(localised.terminal);

        let other_group = parse_entry(
            "group",
            "[Desktop Action new]\nName=New\nExec=x\n[Desktop Entry]\nType=Application\nName=Group\nExec=group\n",
        )
        .expect("only the Desktop Entry group is read");
        assert_eq!(other_group.name, "Group");
    }

    #[test]
    fn exec_splits_quotes_escapes_and_field_codes() {
        assert_eq!(
            split_exec("sh -c \"echo hello world\""),
            ["sh", "-c", "echo hello world"]
        );
        assert_eq!(split_exec("app --path=/a\\ b"), ["app", "--path=/a b"]);
        assert_eq!(split_exec("app %f %U"), ["app"]);
        assert_eq!(split_exec("app 100%%"), ["app", "100%"]);
        assert_eq!(split_exec("app %c %k"), ["app"]);
        assert_eq!(split_exec("app \\\"quoted\\\""), ["app", "\"quoted\""]);
        assert!(split_exec("   ").is_empty());
    }

    #[test]
    fn matching_prefers_exact_then_prefix_then_substring() {
        let entry = |app_id: &str, name: &str| DesktopEntry {
            app_id: app_id.to_string(),
            name: name.to_string(),
            argv: vec![app_id.to_string()],
            terminal: false,
        };
        let entries = vec![
            entry("code", "Code"),
            entry("code-blocks", "Code::Blocks"),
            entry("vscode", "Visual Studio Code"),
            entry("mpv", "mpv"),
        ];

        assert_eq!(
            best_match(&entries, "code").map(|e| e.app_id.as_str()),
            Some("code")
        );
        assert_eq!(
            best_match(&entries, "block").map(|e| e.app_id.as_str()),
            Some("code-blocks")
        );
        assert_eq!(
            best_match(&entries, "studio").map(|e| e.app_id.as_str()),
            Some("vscode")
        );
        assert_eq!(
            best_match(&entries, "MPV").map(|e| e.app_id.as_str()),
            Some("mpv")
        );
        assert!(best_match(&entries, "zzz").is_none());
        assert!(best_match(&entries, "   ").is_none());
    }
}

/// Terminal emulators tried, in order, when `$TERMINAL` is unset or missing.
pub const TERMINALS: &[&str] = &[
    "foot",
    "kitty",
    "alacritty",
    "wezterm",
    "ghostty",
    "konsole",
    "gnome-terminal",
    "xfce4-terminal",
    "xterm",
];

/// Pick the terminal for the `terminal` action: `$TERMINAL` when it names an
/// installed program (its first word, so `$TERMINAL="foot -e"` works), else the
/// first installed entry of [`TERMINALS`]. `installed` answers whether a
/// program name is on `PATH`.
pub fn pick_terminal(
    env_terminal: Option<&str>,
    installed: impl Fn(&str) -> bool,
) -> Option<String> {
    if let Some(preferred) = env_terminal
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let program = preferred.split_whitespace().next().unwrap_or(preferred);
        if installed(program) {
            return Some(preferred.to_string());
        }
    }
    TERMINALS
        .iter()
        .find(|name| installed(name))
        .map(|name| (*name).to_string())
}

#[cfg(test)]
mod terminal_tests {
    use super::*;

    #[test]
    fn terminal_env_wins_when_installed_and_falls_back_otherwise() {
        let installed = |name: &str| matches!(name, "kitty" | "foot");
        assert_eq!(
            pick_terminal(Some("kitty -1"), installed).as_deref(),
            Some("kitty -1")
        );
        assert_eq!(
            pick_terminal(Some("st"), installed).as_deref(),
            Some("foot")
        );
        assert_eq!(pick_terminal(None, installed).as_deref(), Some("foot"));
        assert_eq!(
            pick_terminal(Some("  "), |name| name == "xterm").as_deref(),
            Some("xterm")
        );
        assert_eq!(pick_terminal(None, |_| false), None);
    }
}
