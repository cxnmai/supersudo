#![allow(dead_code)]

use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub const CONFIG_ENV: &str = "SUPERSUDO_CONFIG";

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub general: GeneralConfig,

    #[serde(default)]
    pub prompt: PromptConfig,

    #[serde(default)]
    pub ui: UiConfig,
}

#[derive(Debug, Default, Deserialize)]
pub struct GeneralConfig {
    pub real_sudo: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct PromptConfig {
    pub template: String,
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            template: "Password: ".to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UiConfig {
    pub enabled: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self { enabled: true }
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
