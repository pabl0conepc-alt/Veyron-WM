//! Keeps the configuration example in `README.md` honest: it must parse with
//! the real parser and validation rules.

use veyron_core::{Config, LayoutKind};

#[test]
fn the_readme_configuration_example_is_valid() {
    let readme = include_str!("../../../README.md");
    let start = readme.find("```toml").expect("README has a TOML example") + "```toml".len();
    let rest = &readme[start..];
    let end = rest.find("```").expect("the TOML example is closed");
    let example = &rest[..end];

    let config = Config::from_toml_str(example).expect("the README example parses");
    assert_eq!(config.layout.mode, LayoutKind::Scroll);
    assert_eq!(config.rules.rules().len(), 3);

    let video = config.rules.resolve("mpv", "video.mkv");
    assert!(video.floating);
    assert!(video.blur);
    assert_eq!(video.opacity, 0.95);

    let chat = config.rules.resolve("signal", "Signal");
    assert_eq!(chat.workspace.as_deref(), Some("chat"));
    assert_eq!(chat.layout, Some(LayoutKind::Dwindle));

    let picture = config
        .rules
        .resolve("org.mozilla.firefox", "Picture-in-Picture");
    assert!(picture.floating);
}
