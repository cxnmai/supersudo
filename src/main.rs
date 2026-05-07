mod auth;
mod config;
mod render;

use std::env;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const REAL_SUDO_ENV: &str = "SUPERSUDO_REAL_SUDO";

fn main() {
    let invocation = match parse_supersudo_args() {
        Ok(invocation) => invocation,
        Err(err) => {
            eprintln!("supersudo: {err}");
            std::process::exit(2);
        }
    };

    let loaded_config = match config::load(invocation.config_path) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("supersudo: {err}");
            std::process::exit(2);
        }
    };

    let real_sudo = match find_real_sudo(loaded_config.config.general.real_sudo.as_deref()) {
        Ok(path) => path,
        Err(err) => {
            eprintln!("supersudo: {err}");
            std::process::exit(127);
        }
    };

    if loaded_config.config.input.mode != config::InputMode::Custom {
        if let Err(err) = render::render_pre_prompt(&loaded_config.config, &invocation.sudo_args) {
            eprintln!("supersudo: {err}");
            std::process::exit(2);
        }
    }

    let display_args = invocation.sudo_args.clone();
    let mut sudo_args = with_default_empty_prompt(invocation.sudo_args);

    if loaded_config.config.input.mode == config::InputMode::Custom {
        match auth::credentials_are_cached(&real_sudo) {
            Ok(true) => {
                if let Err(err) = render::render_authenticated_display(&loaded_config.config, &display_args) {
                    eprintln!("supersudo: {err}");
                    std::process::exit(2);
                }
            }
            Ok(false) => {
                let render_password_ui = |password_feedback: &str, state: auth::PromptState, message: &str| {
                    let mut extra = std::collections::HashMap::new();
                    extra.insert("password".to_string(), password_feedback.to_string());
                    extra.insert("error".to_string(), message.to_string());
                    extra.insert("success".to_string(), message.to_string());
                    match state {
                        auth::PromptState::Normal => render::render_display(&loaded_config.config, &display_args, &extra),
                        auth::PromptState::Error => render::render_error_display(&loaded_config.config, &display_args, &extra),
                        auth::PromptState::Success => render::render_success_display(&loaded_config.config, &display_args, &extra)
                            .map(|maybe| maybe.unwrap_or_default()),
                    }
                };

                if let Err(err) = auth::authenticate_custom(
                    &real_sudo,
                    render_password_ui,
                    loaded_config.config.input.feedback_char,
                    loaded_config.config.input.attempts,
                    loaded_config.config.input.error_delay_ms,
                    loaded_config.config.input.success_delay_ms,
                ) {
                    eprintln!("supersudo: {err}");
                    std::process::exit(1);
                }
            }
            Err(err) => {
                eprintln!("supersudo: {err}");
                std::process::exit(1);
            }
        }

        // After custom validation, do not allow sudo to prompt again.
        sudo_args.insert(0, "-n".to_string());
    }

    let err = Command::new(&real_sudo).args(sudo_args).exec();

    // Only reached if exec failed.
    eprintln!(
        "supersudo: failed to execute {}: {err}",
        real_sudo.display()
    );
    std::process::exit(126);
}

struct Invocation {
    config_path: Option<PathBuf>,
    sudo_args: Vec<String>,
}

fn parse_supersudo_args() -> Result<Invocation, String> {
    let mut args = env::args().skip(1);
    let mut config_path = None;
    let mut sudo_args = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--" => {
                sudo_args.extend(args);
                break;
            }
            "--config" => {
                let path = args
                    .next()
                    .ok_or_else(|| "--config requires a path".to_string())?;
                config_path = Some(PathBuf::from(path));
            }
            _ if arg.starts_with("--config=") => {
                config_path = Some(PathBuf::from(arg.trim_start_matches("--config=")));
            }
            _ => {
                sudo_args.push(arg);
                sudo_args.extend(args);
                break;
            }
        }
    }

    Ok(Invocation {
        config_path,
        sudo_args,
    })
}

fn with_default_empty_prompt(args: Vec<String>) -> Vec<String> {
    if has_sudo_prompt_arg(&args) {
        return args;
    }

    let mut prompted_args = Vec::with_capacity(args.len() + 2);
    prompted_args.push("-p".to_string());
    prompted_args.push(String::new());
    prompted_args.extend(args);
    prompted_args
}

fn has_sudo_prompt_arg(args: &[String]) -> bool {
    let mut end_of_sudo_options = false;

    for arg in args {
        if end_of_sudo_options {
            continue;
        }

        if arg == "--" {
            end_of_sudo_options = true;
            continue;
        }

        if arg == "-p" || arg == "--prompt" || arg.starts_with("--prompt=") {
            return true;
        }

        // sudo accepts short options with attached values, e.g. `-pPassword:`.
        if arg.starts_with("-p") && arg.len() > 2 {
            return true;
        }
    }

    false
}

fn find_real_sudo(configured_path: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = configured_path {
        config::validate_real_sudo_path(path)?;
        reject_self_reference(path, "general.real_sudo")?;
        return Ok(path.to_path_buf());
    }

    if let Ok(path) = env::var(REAL_SUDO_ENV) {
        let path = PathBuf::from(path);
        validate_sudo_path(&path, REAL_SUDO_ENV)?;
        return Ok(path);
    }

    let current_exe = env::current_exe().ok().and_then(|p| p.canonicalize().ok());

    for candidate in ["/usr/bin/sudo", "/bin/sudo", "/usr/local/bin/sudo"] {
        let path = PathBuf::from(candidate);
        if !path.is_file() {
            continue;
        }

        if let (Some(current), Ok(candidate_real)) = (&current_exe, path.canonicalize()) {
            if &candidate_real == current {
                continue;
            }
        }

        return Ok(path);
    }

    Err(format!(
        "could not find the real sudo binary; set {REAL_SUDO_ENV}=/path/to/sudo or general.real_sudo in config"
    ))
}

fn validate_sudo_path(path: &Path, source: &str) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!(
            "{source} must be an absolute path, got {}",
            path.display()
        ));
    }

    if !path.is_file() {
        return Err(format!(
            "{source} points to a non-file path: {}",
            path.display()
        ));
    }

    reject_self_reference(path, source)
}

fn reject_self_reference(path: &Path, source: &str) -> Result<(), String> {
    if let (Ok(current), Ok(candidate)) = (env::current_exe(), path.canonicalize()) {
        if let Ok(current) = current.canonicalize() {
            if current == candidate {
                return Err(format!(
                    "{source} points back to supersudo; refusing to recurse"
                ));
            }
        }
    }

    Ok(())
}
