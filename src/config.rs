#![allow(dead_code)]

use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub const CONFIG_ENV: &str = "SUPERSUDO_CONFIG";

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub general: GeneralConfig,

    #[serde(default)]
    pub display: DisplayConfig,

    #[serde(default)]
    pub styles: HashMap<String, String>,

    #[serde(default)]
    pub input: InputConfig,
}

#[derive(Debug, Default, Deserialize)]
pub struct GeneralConfig {
    pub real_sudo: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct InputConfig {
    pub mode: InputMode,
    pub prompt: String,
    pub feedback_char: char,
    pub attempts: u8,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            mode: InputMode::Sudo,
            prompt: "Password: ".to_string(),
            feedback_char: '*',
            attempts: 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputMode {
    Sudo,
    Custom,
}

impl Default for InputMode {
    fn default() -> Self {
        Self::Sudo
    }
}

#[derive(Debug, Deserialize)]
pub struct DisplayConfig {
    pub enabled: bool,
    pub template: String,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            template: String::new(),
        }
    }
}

#[derive(Debug)]
pub struct LoadedConfig {
    pub config: Config,
    pub path: Option<PathBuf>,
}

pub fn load(cli_path: Option<PathBuf>) -> Result<LoadedConfig, String> {
    if let Some(path) = cli_path {
        return load_required(path);
    }

    if let Ok(path) = env::var(CONFIG_ENV) {
        return load_required(PathBuf::from(path));
    }

    for path in default_config_candidates() {
        if path.is_file() {
            return load_required(path);
        }
    }

    Ok(LoadedConfig {
        config: Config::default(),
        path: None,
    })
}

fn load_required(path: PathBuf) -> Result<LoadedConfig, String> {
    let contents = fs::read_to_string(&path)
        .map_err(|err| format!("failed to read config {}: {err}", path.display()))?;

    let config = toml::from_str::<Config>(&contents)
        .map_err(|err| format!("failed to parse config {}: {err}", path.display()))?;

    Ok(LoadedConfig {
        config,
        path: Some(path),
    })
}

fn default_config_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME") {
        paths.push(PathBuf::from(xdg).join("supersudo/config.toml"));
    } else if let Some(home) = home_dir() {
        paths.push(home.join(".config/supersudo/config.toml"));
    }

    paths.push(PathBuf::from("/etc/supersudo/config.toml"));
    paths
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

pub fn validate_real_sudo_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!(
            "general.real_sudo must be an absolute path, got {}",
            path.display()
        ));
    }

    if !path.is_file() {
        return Err(format!(
            "general.real_sudo points to a non-file path: {}",
            path.display()
        ));
    }

    Ok(())
}
