use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

const BEGIN_MARKER: &str = "# >>> supersudo alias >>>";
const END_MARKER: &str = "# <<< supersudo alias <<<";
const ALIAS_BLOCK: &str =
    "# >>> supersudo alias >>>\nalias sudo='supersudo'\n# <<< supersudo alias <<<\n";

pub fn init_alias() -> Result<PathBuf, String> {
    let path = preferred_shell_config()?;
    let existing = fs::read_to_string(&path).unwrap_or_default();

    if existing.contains(BEGIN_MARKER) && existing.contains(END_MARKER) {
        return Ok(path);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create shell config directory {}: {err}",
                parent.display()
            )
        })?;
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| format!("failed to open shell config {}: {err}", path.display()))?;

    if !existing.is_empty() && !existing.ends_with('\n') {
        file.write_all(b"\n")
            .map_err(|err| format!("failed to update shell config {}: {err}", path.display()))?;
    }

    file.write_all(b"\n")
        .and_then(|_| file.write_all(ALIAS_BLOCK.as_bytes()))
        .map_err(|err| format!("failed to write shell config {}: {err}", path.display()))?;

    Ok(path)
}

pub fn remove_alias() -> Result<PathBuf, String> {
    let path = preferred_shell_config()?;
    let existing = fs::read_to_string(&path)
        .map_err(|err| format!("failed to read shell config {}: {err}", path.display()))?;

    let updated = remove_marked_block(&existing)
        .ok_or_else(|| format!("supersudo alias block was not found in {}", path.display()))?;

    fs::write(&path, updated)
        .map_err(|err| format!("failed to write shell config {}: {err}", path.display()))?;

    Ok(path)
}

fn remove_marked_block(contents: &str) -> Option<String> {
    let start = contents.find(BEGIN_MARKER)?;
    let after_start = &contents[start..];
    let end_relative = after_start.find(END_MARKER)?;
    let end = start + end_relative + END_MARKER.len();
    let end = if contents[end..].starts_with('\n') {
        end + 1
    } else {
        end
    };

    let mut updated = String::new();
    updated.push_str(&contents[..start]);
    updated.push_str(&contents[end..]);
    Some(updated)
}

fn preferred_shell_config() -> Result<PathBuf, String> {
    if let Ok(path) = env::var("SUPERSUDO_SHELL_CONFIG") {
        return Ok(PathBuf::from(path));
    }

    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "could not determine shell config path: HOME is not set".to_string())?;

    let shell = env::var("SHELL").unwrap_or_default();
    let shell_name = shell.rsplit('/').next().unwrap_or_default();

    let rc = match shell_name {
        "zsh" => ".zshrc",
        "fish" => {
            return Err(
                "fish is not supported yet; add `alias sudo='supersudo'` manually".to_string(),
            );
        }
        "bash" => ".bashrc",
        _ => ".profile",
    };

    Ok(home.join(rc))
}
