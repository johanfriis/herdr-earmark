//! User config: `config.toml` in the plugin config directory.

use std::path::Path;

use serde::Deserialize;

pub const DEFAULT_TAB_NAME: &str = "review";

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Label of the tab the review opens in.
    pub tab_name: String,
    /// Branch treated as the mainline. Unset: `origin/HEAD`, then `main`, then `master`.
    pub base_branch: Option<String>,
    /// Clear the delivered comments from the tuicr session (review marks are kept),
    /// so the next review does not paste them again.
    pub clear_after_send: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tab_name: DEFAULT_TAB_NAME.to_string(),
            base_branch: None,
            clear_after_send: true,
        }
    }
}

impl Config {
    /// Loads `<dir>/config.toml`. A missing file yields defaults; a broken one
    /// warns on stderr and yields defaults so a typo never blocks a review.
    pub fn load(dir: Option<&Path>) -> Self {
        let Some(path) = dir.map(|d| d.join("config.toml")) else {
            return Self::default();
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                eprintln!(
                    "herdr-earmark: cannot read {}: {e}; using defaults",
                    path.display()
                );
                return Self::default();
            }
        };
        match Self::parse(&text) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!(
                    "herdr-earmark: invalid {}: {e}; using defaults",
                    path.display()
                );
                Self::default()
            }
        }
    }

    fn parse(text: &str) -> Result<Self, toml::de::Error> {
        let mut cfg: Self = toml::from_str(text)?;
        if cfg.tab_name.trim().is_empty() {
            cfg.tab_name = DEFAULT_TAB_NAME.to_string();
        }
        cfg.base_branch = cfg
            .base_branch
            .map(|b| b.trim().to_string())
            .filter(|b| !b.is_empty());
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_values_fall_back_to_defaults() {
        let cfg = Config::parse("tab_name = \"  \"\nbase_branch = \"\"").unwrap();
        assert_eq!(cfg.tab_name, DEFAULT_TAB_NAME);
        assert_eq!(cfg.base_branch, None);
        assert!(cfg.clear_after_send);
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(Config::parse("tabname = \"x\"").is_err());
    }
}
