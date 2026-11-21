//! Keeps the configuration examples in `README.md` honest: every TOML block
//! must parse with the real parser and validation rules, and the full example
//! must mean what the README says it means.

use veyron_core::{Config, LayoutKind};

/// Every fenced ```toml block in the README, in order.
fn toml_blocks(readme: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut rest = readme;
    while let Some(start) = rest.find("```toml") {
        let body = &rest[start + "```toml".len()..];
        let end = body.find("```").expect("every TOML example is closed");
        blocks.push(&body[..end]);
        rest = &body[end + 3..];
    }
    blocks
}

#[test]
fn every_readme_toml_example_is_valid() {
    let readme = include_str!("../../../README.md");
    let blocks = toml_blocks(readme);
    assert!(!blocks.is_empty(), "README has TOML examples");
    for (index, block) in blocks.iter().enumerate() {
        if let Err(error) = Config::from_toml_str(block) {
            panic!("README TOML example #{index} is invalid: {error}\n{block}");
        }
    }
}

#[test]
fn the_full_readme_configuration_example_means_what_it_says() {
    let readme = include_str!("../../../README.md");
    let example = toml_blocks(readme)
        .into_iter()
        .find(|block| block.contains("[[rules]]"))
        .expect("README has the full configuration example");

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
