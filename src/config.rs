#![allow(dead_code)]

use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub const CONFIG_ENV: &str = "SUPERSUDO_CONFIG";
const MAX_EXTERNAL_FILE_BYTES: u64 = 1024 * 1024;

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

    #[serde(default)]
    pub animations: HashMap<String, PathBuf>,

    #[serde(default)]
    pub animation_speeds: HashMap<String, u64>,

    #[serde(skip)]
    pub loaded_animations: HashMap<String, LoadedAnimation>,
}

#[derive(Debug, Clone)]
pub struct LoadedAnimation {
    pub frames: Vec<String>,
    pub speed_ms: u64,
}

#[derive(Debug, Default, Deserialize)]
pub struct GeneralConfig {
    pub real_sudo: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct InputConfig {
    pub mode: InputMode,
    pub feedback_char: char,
    pub attempts: u8,
    pub error_delay_ms: u64,
    pub success_delay_ms: u64,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            mode: InputMode::Sudo,
            feedback_char: '*',
            attempts: 3,
            error_delay_ms: 900,
            success_delay_ms: 700,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputMode {
    #[default]
    Sudo,
    Custom,
}

#[derive(Debug, Default, Deserialize)]
pub struct DisplayConfig {
    pub enabled: bool,
    #[serde(default)]
    pub template: String,
    pub template_file: Option<PathBuf>,
    pub error_template: Option<String>,
    pub error_template_file: Option<PathBuf>,
    pub success_template: Option<String>,
    pub success_template_file: Option<PathBuf>,
    pub authenticated_template: Option<String>,
    pub authenticated_template_file: Option<PathBuf>,
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

    let mut config = toml::from_str::<Config>(&contents)
        .map_err(|err| format!("failed to parse config {}: {err}", path.display()))?;

    load_external_templates(&mut config, &path)?;
    load_external_animations(&mut config, &path)?;

    Ok(LoadedConfig {
        config,
        path: Some(path),
    })
}

fn load_external_animations(config: &mut Config, config_path: &Path) -> Result<(), String> {
    for (name, path) in &config.animations {
        let contents = read_template_file(config_path, path, &format!("animations.{name}"))?;
        let frames = parse_animation_frames(&contents);
        if frames.is_empty() {
            return Err(format!("animation `{name}` has no frames"));
        }

        config.loaded_animations.insert(
            name.clone(),
            LoadedAnimation {
                frames,
                speed_ms: config.animation_speeds.get(name).copied().unwrap_or(120),
            },
        );
    }

    Ok(())
}

fn parse_animation_frames(contents: &str) -> Vec<String> {
    let delimiter = "\n---\n";
    if contents.contains(delimiter) {
        contents
            .split(delimiter)
            .map(|frame| frame.trim_matches('\n').to_string())
            .filter(|frame| !frame.is_empty())
            .collect()
    } else {
        contents
            .lines()
            .map(str::to_string)
            .filter(|frame| !frame.is_empty())
            .collect()
    }
}

fn load_external_templates(config: &mut Config, config_path: &Path) -> Result<(), String> {
    if let Some(path) = &config.display.template_file {
        config.display.template = read_template_file(config_path, path, "display.template_file")?;
    }

    if let Some(path) = &config.display.error_template_file {
        config.display.error_template = Some(read_template_file(
            config_path,
            path,
            "display.error_template_file",
        )?);
    }

    if let Some(path) = &config.display.success_template_file {
        config.display.success_template = Some(read_template_file(
            config_path,
            path,
            "display.success_template_file",
        )?);
    }

    if let Some(path) = &config.display.authenticated_template_file {
        config.display.authenticated_template = Some(read_template_file(
            config_path,
            path,
            "display.authenticated_template_file",
        )?);
    }

    Ok(())
}

fn read_template_file(
    config_path: &Path,
    template_path: &Path,
    field: &str,
) -> Result<String, String> {
    let path = if template_path.is_absolute() {
        template_path.to_path_buf()
    } else {
        config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(template_path)
    };

    let metadata = fs::metadata(&path).map_err(|err| {
        format!(
            "failed to inspect {field} {} resolved from config {}: {err}",
            path.display(),
            config_path.display()
        )
    })?;

    if metadata.len() > MAX_EXTERNAL_FILE_BYTES {
        return Err(format!(
            "refusing to read {field} {}: file is larger than {} bytes",
            path.display(),
            MAX_EXTERNAL_FILE_BYTES
        ));
    }

    fs::read_to_string(&path).map_err(|err| {
        format!(
            "failed to read {field} {} resolved from config {}: {err}",
            path.display(),
            config_path.display()
        )
    })
}

pub fn default_user_config_path() -> Result<PathBuf, String> {
    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(xdg).join("supersudo/config.toml"));
    }

    if let Some(home) = home_dir() {
        return Ok(home.join(".config/supersudo/config.toml"));
    }

    Err("could not determine user config path: neither XDG_CONFIG_HOME nor HOME is set".to_string())
}

fn default_config_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(path) = default_user_config_path() {
        paths.push(path);
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
