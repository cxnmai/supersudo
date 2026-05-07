mod config;

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

    // Later, customization will happen before this point:
    // - inspect invocation.sudo_args to build template variables
    // - render loaded_config.config.prompt.template
    // - add `-p <prompt>` when appropriate
    // For now we are intentionally transparent: pass every sudo argument through.
    let err = Command::new(&real_sudo).args(invocation.sudo_args).exec();

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
