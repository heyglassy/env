use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
};

use libghostty_vt::style::RgbColor;
use tracing::warn;

use crate::theme::parse_hex_rgb;

#[derive(Clone, Debug)]
pub(crate) struct Config {
    pub(crate) status_label: String,
    pub(crate) status_cwd: Option<String>,
    pub(crate) status_foreground: Option<RgbColor>,
    pub(crate) status_background: Option<RgbColor>,
    pub(crate) status_segments: Vec<StatusSegment>,
    pub(crate) time_format: String,
    pub(crate) child_term: String,
    pub(crate) shell_integration: bool,
    pub(crate) theme_query: bool,
    pub(crate) osc_theme_query: bool,
    pub(crate) appearance: AppearancePreference,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StatusSegment {
    Label,
    Cwd,
    Git,
    Time,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AppearancePreference {
    Auto,
    Light,
    Dark,
}

impl Config {
    pub(crate) fn load() -> Self {
        let path = config_path();
        let config_text = path
            .as_deref()
            .and_then(|path| match fs::read_to_string(path) {
                Ok(config) => Some(config),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    warn!(%error, path = %path.display(), "failed to read glassterm config");
                    None
                }
            });

        Self::from_sources(
            config_text.as_deref(),
            |key| env::var(key).ok(),
            env::var("TERM").ok(),
        )
    }

    fn from_sources<F>(config_text: Option<&str>, env_value: F, term: Option<String>) -> Self
    where
        F: Fn(&str) -> Option<String>,
    {
        let mut config = Self::defaults(term);
        let mut unknown_keys = HashSet::new();

        if let Some(config_text) = config_text {
            for (key, value) in key_value_lines(config_text) {
                match key {
                    "status-label" => config.status_label = value.to_string(),
                    "status-foreground" => update_color(value, &mut config.status_foreground, key),
                    "status-background" => update_color(value, &mut config.status_background, key),
                    "status-segments" => config.status_segments = parse_status_segments(value),
                    "time-format" => config.time_format = value.to_string(),
                    "child-term" => config.child_term = value.to_string(),
                    "shell-integration" => update_bool(value, &mut config.shell_integration, key),
                    "theme-query" => update_bool(value, &mut config.theme_query, key),
                    "osc-theme-query" => update_bool(value, &mut config.osc_theme_query, key),
                    "appearance" => update_appearance(value, &mut config.appearance, key),
                    _ if unknown_keys.insert(key.to_string()) => {
                        warn!(key, "unknown glassterm config key");
                    }
                    _ => {}
                }
            }
        }

        if let Some(value) = env_value("GLASSTERM_STATUS_LABEL") {
            config.status_label = value;
        }
        config.status_cwd = env_value("GLASSTERM_STATUS_CWD");
        if let Some(value) = env_value("GLASSTERM_STATUS_FOREGROUND") {
            update_color(
                &value,
                &mut config.status_foreground,
                "GLASSTERM_STATUS_FOREGROUND",
            );
        }
        if let Some(value) = env_value("GLASSTERM_STATUS_BACKGROUND") {
            update_color(
                &value,
                &mut config.status_background,
                "GLASSTERM_STATUS_BACKGROUND",
            );
        }
        if let Some(value) = env_value("GLASSTERM_CHILD_TERM") {
            config.child_term = value;
        }
        if env_value("GLASSTERM_DISABLE_SHELL_INTEGRATION").is_some() {
            config.shell_integration = false;
        }
        if env_value("GLASSTERM_DISABLE_THEME_QUERY").is_some() {
            config.theme_query = false;
        }
        if env_value("GLASSTERM_DISABLE_OSC_THEME_QUERY").is_some() {
            config.osc_theme_query = false;
        }
        if let Some(value) = env_value("GLASSTERM_GHOSTTY_APPEARANCE") {
            update_appearance(
                &value,
                &mut config.appearance,
                "GLASSTERM_GHOSTTY_APPEARANCE",
            );
        }

        config
    }

    fn defaults(term: Option<String>) -> Self {
        Self {
            status_label: "glassterm".to_string(),
            status_cwd: None,
            status_foreground: None,
            status_background: None,
            status_segments: default_status_segments(),
            time_format: "%H:%M".to_string(),
            child_term: term.unwrap_or_else(|| "xterm-ghostty".to_string()),
            shell_integration: true,
            theme_query: true,
            osc_theme_query: true,
            appearance: AppearancePreference::Auto,
        }
    }
}

fn config_path() -> Option<PathBuf> {
    if let Some(config_home) = env::var_os("XDG_CONFIG_HOME") {
        return Some(Path::new(&config_home).join("glassterm/config"));
    }
    env::var_os("HOME").map(|home| Path::new(&home).join(".config/glassterm/config"))
}

pub(crate) fn key_value_lines(input: &str) -> impl Iterator<Item = (&str, &str)> {
    input.lines().filter_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }

        let (key, value) = line.split_once('=')?;
        Some((key.trim(), value.trim()))
    })
}

fn default_status_segments() -> Vec<StatusSegment> {
    vec![
        StatusSegment::Label,
        StatusSegment::Cwd,
        StatusSegment::Git,
        StatusSegment::Time,
    ]
}

pub(crate) fn parse_status_segments(value: &str) -> Vec<StatusSegment> {
    let mut segments = Vec::new();
    let mut unknown_segments = HashSet::new();

    for segment in value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        match segment {
            "label" => segments.push(StatusSegment::Label),
            "cwd" => segments.push(StatusSegment::Cwd),
            "git" => segments.push(StatusSegment::Git),
            "time" => segments.push(StatusSegment::Time),
            _ if unknown_segments.insert(segment.to_string()) => {
                warn!(segment, "unknown glassterm status segment");
            }
            _ => {}
        }
    }

    segments
}

fn update_color(value: &str, target: &mut Option<RgbColor>, key: &str) {
    if let Some(color) = parse_hex_rgb(value) {
        *target = Some(color);
    } else {
        warn!(key, value, "invalid glassterm color");
    }
}

fn update_bool(value: &str, target: &mut bool, key: &str) {
    if let Some(value) = parse_bool(value) {
        *target = value;
    } else {
        warn!(key, value, "invalid glassterm boolean");
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

fn update_appearance(value: &str, target: &mut AppearancePreference, key: &str) {
    match value.trim() {
        "auto" => *target = AppearancePreference::Auto,
        "light" => *target = AppearancePreference::Light,
        "dark" => *target = AppearancePreference::Dark,
        _ => warn!(key, value, "invalid glassterm appearance"),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn config_from(config: &str, envs: &[(&str, &str)]) -> Config {
        let envs = envs.iter().copied().collect::<HashMap<_, _>>();
        Config::from_sources(
            Some(config),
            |key| envs.get(key).map(|value| value.to_string()),
            Some("host-term".to_string()),
        )
    }

    #[test]
    fn config_precedence_is_env_then_file_then_default() {
        let config = config_from(
            r#"
status-label = from-file
child-term = file-term
time-format = %S
"#,
            &[
                ("GLASSTERM_STATUS_LABEL", "from-env"),
                ("GLASSTERM_CHILD_TERM", "env-term"),
            ],
        );

        assert_eq!(config.status_label, "from-env");
        assert_eq!(config.child_term, "env-term");
        assert_eq!(config.time_format, "%S");
        assert_eq!(config.status_segments, default_status_segments());
    }

    #[test]
    fn config_parses_bool_forms() {
        let config = config_from(
            r#"
shell-integration = no
theme-query = 0
osc-theme-query = false
"#,
            &[],
        );

        assert!(!config.shell_integration);
        assert!(!config.theme_query);
        assert!(!config.osc_theme_query);

        let config = config_from(
            r#"
shell-integration = yes
theme-query = 1
osc-theme-query = true
"#,
            &[],
        );

        assert!(config.shell_integration);
        assert!(config.theme_query);
        assert!(config.osc_theme_query);
    }

    #[test]
    fn config_ignores_unknown_keys() {
        let config = config_from(
            r#"
unknown-key = value
unknown-key = other
status-label = kept
"#,
            &[],
        );

        assert_eq!(config.status_label, "kept");
    }

    #[test]
    fn segment_list_parsing_skips_unknown_segments() {
        assert_eq!(
            parse_status_segments("time, nope, git, label, nope"),
            vec![
                StatusSegment::Time,
                StatusSegment::Git,
                StatusSegment::Label
            ]
        );
    }
}
