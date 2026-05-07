use crate::config::Config;
use std::collections::HashMap;
use std::env;
use std::io::{self, IsTerminal, Write};
use unicode_width::UnicodeWidthStr;

pub fn render_display(
    config: &Config,
    sudo_args: &[String],
    extra_vars: &HashMap<String, String>,
) -> Result<String, String> {
    render_display_at(config, sudo_args, extra_vars, 0)
}

pub fn render_display_at(
    config: &Config,
    sudo_args: &[String],
    extra_vars: &HashMap<String, String>,
    elapsed_ms: u128,
) -> Result<String, String> {
    render_named_template(
        &config.display.template,
        config,
        sudo_args,
        extra_vars,
        elapsed_ms,
    )
}

pub fn render_error_display_at(
    config: &Config,
    sudo_args: &[String],
    extra_vars: &HashMap<String, String>,
    elapsed_ms: u128,
) -> Result<String, String> {
    let template = config
        .display
        .error_template
        .as_deref()
        .unwrap_or(&config.display.template);
    render_named_template(template, config, sudo_args, extra_vars, elapsed_ms)
}

pub fn render_success_display_at(
    config: &Config,
    sudo_args: &[String],
    extra_vars: &HashMap<String, String>,
    elapsed_ms: u128,
) -> Result<Option<String>, String> {
    let Some(template) = config.display.success_template.as_deref() else {
        return Ok(None);
    };
    render_named_template(template, config, sudo_args, extra_vars, elapsed_ms).map(Some)
}

pub fn render_authenticated_display(config: &Config, sudo_args: &[String]) -> Result<(), String> {
    if !config.display.enabled || !io::stdout().is_terminal() {
        return Ok(());
    }

    let Some(template) = &config.display.authenticated_template else {
        return Ok(());
    };

    let rendered = render_named_template(template, config, sudo_args, &HashMap::new(), 0)?;
    write_rendered(&rendered)
}

fn render_named_template(
    template: &str,
    config: &Config,
    sudo_args: &[String],
    extra_vars: &HashMap<String, String>,
    elapsed_ms: u128,
) -> Result<String, String> {
    let mut vars = template_vars(sudo_args);
    vars.extend(animation_vars(config, elapsed_ms));
    vars.extend(extra_vars.clone());
    render_template(template, &config.styles, &vars)
}

pub fn render_pre_prompt(config: &Config, sudo_args: &[String]) -> Result<(), String> {
    if !config.display.enabled || !io::stdout().is_terminal() {
        return Ok(());
    }

    let rendered = render_display(config, sudo_args, &HashMap::new())?;

    write_rendered(&rendered)
}

fn write_rendered(rendered: &str) -> Result<(), String> {
    let mut stdout = io::stdout();
    stdout
        .write_all(rendered.as_bytes())
        .map_err(|err| format!("failed to write display template: {err}"))?;

    if !rendered.ends_with('\n') {
        stdout
            .write_all(b"\n")
            .map_err(|err| format!("failed to write display newline: {err}"))?;
    }

    stdout
        .flush()
        .map_err(|err| format!("failed to flush display: {err}"))?;

    Ok(())
}

fn animation_vars(config: &Config, elapsed_ms: u128) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    for (name, animation) in &config.loaded_animations {
        let frame_index = if animation.speed_ms == 0 {
            0
        } else {
            ((elapsed_ms / animation.speed_ms as u128) as usize) % animation.frames.len()
        };
        vars.insert(
            format!("animation:{name}"),
            animation.frames[frame_index].clone(),
        );
    }
    vars
}

fn template_vars(sudo_args: &[String]) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    vars.insert("user".to_string(), env::var("USER").unwrap_or_default());
    vars.insert("host".to_string(), hostname());
    vars.insert(
        "cwd".to_string(),
        env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
    );
    vars.insert("command".to_string(), shell_join(sudo_args));
    vars
}

fn hostname() -> String {
    env::var("HOSTNAME")
        .ok()
        .filter(|host| !host.is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|host| host.trim().to_string())
        })
        .unwrap_or_default()
}

fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "@%_+=:,./-".contains(c))
            {
                arg.clone()
            } else {
                format!("'{}'", arg.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn render_template(
    template: &str,
    styles: &HashMap<String, String>,
    vars: &HashMap<String, String>,
) -> Result<String, String> {
    let mut out = String::new();
    let mut chars = template.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '{' {
            out.push(ch);
            continue;
        }

        let mut token = String::new();
        let mut closed = false;
        for next in chars.by_ref() {
            if next == '}' {
                closed = true;
                break;
            }
            token.push(next);
        }

        if !closed {
            out.push('{');
            out.push_str(&token);
            break;
        }

        if token == "reset" {
            out.push_str("\x1b[0m");
        } else if let Some(name) = token.strip_prefix("style:") {
            let style = styles
                .get(name)
                .ok_or_else(|| format!("unknown style `{name}` in display template"))?;
            out.push_str(&style_to_ansi(style)?);
        } else if let Some(value) = render_value_token(&token, vars)? {
            out.push_str(&value);
        } else {
            return Err(format!(
                "unknown variable `{{{token}}}` in display template"
            ));
        }
    }

    out.push_str("\x1b[0m");
    Ok(out)
}

fn render_value_token(
    token: &str,
    vars: &HashMap<String, String>,
) -> Result<Option<String>, String> {
    if let Some((name, width)) = token.rsplit_once(":pad=") {
        let width = parse_pad_width(token, width)?;
        if let Some(value) = vars.get(name) {
            return Ok(Some(pad_or_truncate(value, width)));
        }
    }

    if let Some(rest) = token.strip_prefix("lit:") {
        let Some((literal, width)) = rest.rsplit_once(":pad=") else {
            return Ok(Some(rest.to_string()));
        };
        let width = parse_pad_width(token, width)?;
        return Ok(Some(pad_or_truncate(literal, width)));
    }

    Ok(vars.get(token).cloned())
}

fn parse_pad_width(token: &str, width: &str) -> Result<usize, String> {
    width
        .parse::<usize>()
        .map_err(|_| format!("invalid pad width in `{{{token}}}`"))
}

fn pad_or_truncate(value: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;

    for ch in value.chars() {
        let ch_width = ch.to_string().width();
        if used + ch_width > width {
            break;
        }
        out.push(ch);
        used += ch_width;
    }

    if used < value.width() && width > 0 {
        while out.width() + 1 > width {
            out.pop();
        }
        if out.width() < width {
            out.push('…');
        }
    }

    let padding = width.saturating_sub(out.width());
    out.push_str(&" ".repeat(padding));
    out
}

fn style_to_ansi(style: &str) -> Result<String, String> {
    let mut codes = Vec::new();

    for token in style.split_whitespace() {
        let code = match token {
            "reset" => 0,
            "bold" => 1,
            "dim" => 2,
            "italic" => 3,
            "underline" => 4,
            "reverse" => 7,
            "black" => 30,
            "red" => 31,
            "green" => 32,
            "yellow" => 33,
            "blue" => 34,
            "magenta" => 35,
            "cyan" => 36,
            "white" => 37,
            "bright_black" => 90,
            "bright_red" => 91,
            "bright_green" => 92,
            "bright_yellow" => 93,
            "bright_blue" => 94,
            "bright_magenta" => 95,
            "bright_cyan" => 96,
            "bright_white" => 97,
            _ if token.starts_with("bg:") => bg_code(&token[3..])?,
            _ => return Err(format!("unknown style token `{token}`")),
        };
        codes.push(code.to_string());
    }

    if codes.is_empty() {
        return Ok(String::new());
    }

    Ok(format!("\x1b[{}m", codes.join(";")))
}

fn bg_code(color: &str) -> Result<i32, String> {
    match color {
        "black" => Ok(40),
        "red" => Ok(41),
        "green" => Ok(42),
        "yellow" => Ok(43),
        "blue" => Ok(44),
        "magenta" => Ok(45),
        "cyan" => Ok(46),
        "white" => Ok(47),
        "bright_black" => Ok(100),
        "bright_red" => Ok(101),
        "bright_green" => Ok(102),
        "bright_yellow" => Ok(103),
        "bright_blue" => Ok(104),
        "bright_magenta" => Ok(105),
        "bright_cyan" => Ok(106),
        "bright_white" => Ok(107),
        _ => Err(format!("unknown background color `{color}`")),
    }
}
