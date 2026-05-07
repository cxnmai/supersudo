use crate::{cli, config, shell};
use std::io::{self, Write};

pub fn run() -> Result<(), String> {
    println!("supersudo setup");
    println!();

    let config_path = config::default_user_config_path()?;
    if config_path.exists() {
        println!("Config already exists: {}", config_path.display());
        if ask_yes_no("Overwrite it?", false)? {
            let path = cli::init_config(true)?;
            println!("Wrote config: {}", path.display());
        } else {
            println!("Skipped config creation.");
        }
    } else if ask_yes_no(
        &format!("Create default config at {}?", config_path.display()),
        true,
    )? {
        let path = cli::init_config(false)?;
        println!("Created config: {}", path.display());
    } else {
        println!("Skipped config creation.");
    }

    println!();

    if ask_yes_no("Alias `sudo` to `supersudo` in your shell config?", false)? {
        let path = shell::init_alias()?;
        println!("Added sudo alias to: {}", path.display());
        println!("Restart your shell or source the file for it to take effect.");
    } else {
        println!("Skipped shell alias setup.");
    }

    println!();
    println!("Setup complete.");
    Ok(())
}

fn ask_yes_no(prompt: &str, default: bool) -> Result<bool, String> {
    let suffix = if default { "[Y/n]" } else { "[y/N]" };

    loop {
        print!("{prompt} {suffix} ");
        io::stdout()
            .flush()
            .map_err(|err| format!("failed to flush prompt: {err}"))?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(|err| format!("failed to read input: {err}"))?;

        let input = input.trim().to_ascii_lowercase();
        match input.as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => println!("Please answer yes or no."),
        }
    }
}
