use std::env;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const REAL_SUDO_ENV: &str = "SUPERSUDO_REAL_SUDO";

fn main() {
    let real_sudo = match find_real_sudo() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("supersudo: {err}");
            std::process::exit(127);
        }
    };

    let args: Vec<String> = env::args().skip(1).collect();

    // Later, customization will happen before this point:
    // - inspect args to build template variables
    // - render prompt
    // - add `-p <prompt>` when appropriate
    // For now we are intentionally transparent: pass every argument through.
    let err = Command::new(&real_sudo).args(args).exec();

    // Only reached if exec failed.
    eprintln!(
        "supersudo: failed to execute {}: {err}",
        real_sudo.display()
    );
    std::process::exit(126);
}

fn find_real_sudo() -> Result<PathBuf, String> {
    if let Ok(path) = env::var(REAL_SUDO_ENV) {
        let path = PathBuf::from(path);
        validate_sudo_path(&path)?;
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
        "could not find the real sudo binary; set {REAL_SUDO_ENV}=/path/to/sudo"
    ))
}

fn validate_sudo_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!(
            "{REAL_SUDO_ENV} must be an absolute path, got {}",
            path.display()
        ));
    }

    if !path.is_file() {
        return Err(format!(
            "{REAL_SUDO_ENV} points to a non-file path: {}",
            path.display()
        ));
    }

    if let (Ok(current), Ok(candidate)) = (env::current_exe(), path.canonicalize()) {
        if let Ok(current) = current.canonicalize() {
            if current == candidate {
                return Err(format!(
                    "{REAL_SUDO_ENV} points back to supersudo; refusing to recurse"
                ));
            }
        }
    }

    Ok(())
}
