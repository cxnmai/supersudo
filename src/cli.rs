use crate::config;
use std::fs;
use std::path::PathBuf;

pub enum CliAction {
    Run {
        config_path: Option<PathBuf>,
        sudo_args: Vec<String>,
    },
    ConfigInit {
        force: bool,
    },
    PathInit,
    PathRemove,
    Setup,
    Help,
}

pub fn parse_args(mut args: impl Iterator<Item = String>) -> Result<CliAction, String> {
    let mut config_path = None;
    let mut sudo_args = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" | "help" => return Ok(CliAction::Help),
            "config" => return parse_config_command(args),
            "path" => return parse_path_command(args),
            "setup" => return Ok(CliAction::Setup),
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

    Ok(CliAction::Run {
        config_path,
        sudo_args,
    })
}

fn parse_path_command(mut args: impl Iterator<Item = String>) -> Result<CliAction, String> {
    let Some(command) = args.next() else {
        return Err("expected path subcommand, e.g. `supersudo path init`".to_string());
    };

    match command.as_str() {
        "-h" | "--help" | "help" => Ok(CliAction::Help),
        "init" => Ok(CliAction::PathInit),
        "remove" => Ok(CliAction::PathRemove),
        _ => Err(format!("unknown path subcommand `{command}`")),
    }
}

fn parse_config_command(mut args: impl Iterator<Item = String>) -> Result<CliAction, String> {
    let Some(command) = args.next() else {
        return Err("expected config subcommand, e.g. `supersudo config init`".to_string());
    };

    match command.as_str() {
        "-h" | "--help" | "help" => Ok(CliAction::Help),
        "init" => {
            let mut force = false;
            for arg in args {
                match arg.as_str() {
                    "--force" | "-f" => force = true,
                    _ => return Err(format!("unknown config init option `{arg}`")),
                }
            }
            Ok(CliAction::ConfigInit { force })
        }
        _ => Err(format!("unknown config subcommand `{command}`")),
    }
}

pub fn init_config(force: bool) -> Result<PathBuf, String> {
    let path = config::default_user_config_path()?;

    if path.exists() && !force {
        return Err(format!(
            "config already exists: {}\nuse `supersudo config init --force` to overwrite it",
            path.display()
        ));
    }

    let parent = path
        .parent()
        .ok_or_else(|| format!("invalid config path: {}", path.display()))?;

    fs::create_dir_all(parent).map_err(|err| {
        format!(
            "failed to create config directory {}: {err}",
            parent.display()
        )
    })?;

    fs::write(&path, DEFAULT_CONFIG)
        .map_err(|err| format!("failed to write config file {}: {err}", path.display()))?;

    Ok(path)
}

pub fn print_help() {
    println!(
        r#"supersudo

USAGE:
    supersudo [--config <path>] [--] <sudo args...>
    supersudo <command>

COMMANDS:
    setup                 Interactive setup for config and sudo alias
    config init           Create default user config
    config init --force   Overwrite default user config
    path init             Add `alias sudo='supersudo'` to shell config
    path remove           Remove supersudo alias block from shell config
    help, --help          Show this help

OPTIONS:
    --config <path>       Use a specific config file
    --config=<path>       Use a specific config file

EXAMPLES:
    supersudo whoami
    supersudo --config examples/config.toml -- whoami
    supersudo config init
    supersudo setup
"#
    );
}

const DEFAULT_CONFIG: &str = r#"# Supersudo configuration

[general]
# Path to the real sudo binary. Keep this absolute to avoid recursion if sudo
# is aliased or shimmed to supersudo.
real_sudo = "/usr/bin/sudo"

[input]
# "sudo" lets real sudo read the password.
# "custom" lets supersudo read the password for live feedback/animations. It
# uses protected memory, but "sudo" mode is safer because supersudo never sees
# the password.
mode = "sudo"
feedback_char = "*"
attempts = 3
error_delay_ms = 900
success_delay_ms = 700

[display]
enabled = true
template = """
{style:title}Authentication required{reset}
{style:label}command:{reset} {style:value}{command}{reset}
"""

authenticated_template = "{style:ok}Authenticated!{reset} {style:label}using cached sudo credentials for{reset} {style:value}{command}{reset}"

[styles]
title = "bold yellow"
label = "dim white"
value = "bright_white"
ok = "bold bright_green"
"#;
