//! Strict project tooling configuration; never part of executable input identity.

use crate::CliError;
use serde::Deserialize;
use std::{fs, io::ErrorKind, path::Path};
use toml::from_str;

pub(crate) const CONFIG_NAME: &str = "Kagari.tools.toml";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct ToolConfig {
    pub format: FormatConfig,
    pub lint: LintConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct FormatConfig {
    pub indent_width: usize,
    pub max_width: usize,
    pub newline: Newline,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            indent_width: 4,
            max_width: 100,
            newline: Newline::Preserve,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Newline {
    #[default]
    Preserve,
    Lf,
    Crlf,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Level {
    Allow,
    Warn,
    Deny,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct LintConfig {
    pub unused_binding: Option<Level>,
    pub unused_parameter: Option<Level>,
    pub unnecessary_var: Option<Level>,
    pub non_snake_case: Option<Level>,
    pub non_upper_camel_case: Option<Level>,
    pub non_upper_case_globals: Option<Level>,
}

impl ToolConfig {
    pub fn load(root: &Path) -> Result<Self, CliError> {
        let path = root.join(CONFIG_NAME);
        match fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text)
                .map_err(|message| CliError::message(2, format!("{}: {message}", path.display()))),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(CliError::message(2, format!("{}: {error}", path.display()))),
        }
    }

    fn parse(text: &str) -> Result<Self, String> {
        let config: Self = from_str(text).map_err(|error| error.to_string())?;
        if !(1..=16).contains(&config.format.indent_width)
            || !(20..=1000).contains(&config.format.max_width)
        {
            return Err("indent_width must be 1..=16 and max_width must be 20..=1000".into());
        }
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_is_strict_and_style_only() {
        let config = ToolConfig::parse(
            "[format]\nindent_width=2\nnewline='crlf'\n[lint]\nunused_binding='deny'",
        )
        .unwrap();
        assert_eq!(config.format.indent_width, 2);
        assert!(matches!(config.lint.unused_binding, Some(Level::Deny)));
        for text in [
            "unknown=1",
            "[format]\nwidth=80",
            "[format]\nindent_width=0",
            "[lint]\nunused='allow'",
            "[lint]\nunused_binding='off'",
        ] {
            assert!(ToolConfig::parse(text).is_err(), "{text}");
        }
    }
}
