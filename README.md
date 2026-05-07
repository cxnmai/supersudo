# supersudo

`supersudo` is an experimental Rust wrapper around the user's existing `sudo` installation. It provides configurable terminal UI, live password feedback, prompt states, and animations while delegating authentication, sudoers policy, credential caching, and command execution to the system `sudo` binary.

<img width="1048" height="968" alt="githubgif1" src="https://github.com/user-attachments/assets/1cb40070-ef5d-4421-a375-b0d8a0a514e1" />


<img width="1076" height="900" alt="githubgif2" src="https://github.com/user-attachments/assets/1ff6bb13-bf9d-400b-ac40-672af860287b" />


## Security Notice

`supersudo` does **not** replace sudo. It delegates authentication, sudoers policy, credential caching, and command execution to the real system `sudo`.

Risk summary:

- `mode = "sudo"` is safest because `supersudo` never sees your password.
- `mode = "custom"` enables live password UI, but `supersudo` must read your password.
- Custom mode uses protected memory, avoids growable heap password buffers, zeroes password bytes after use, and marks the process non-dumpable on Linux while reading the password.
- These protections are best-effort. They do not protect against a compromised process, root/debugger access, malicious terminal behavior, kernel/terminal buffers, sudo internals, or abnormal termination such as `SIGKILL`.
- Do not use untrusted configs or templates; they can display arbitrary terminal text.

Security flow:

- In sudo mode, `supersudo` renders optional UI, then execs real `sudo`.
- In custom mode, `supersudo` reads the password into protected memory, validates it with `sudo -S -p "" -v`, clears it, then runs the requested command with `sudo -n`.
- The password is never passed through args, env vars, config files, temp files, or shell commands.

## Usage

Run a command through supersudo:

```bash
supersudo whoami
```

Use a specific config:

```bash
supersudo --config examples/config.toml -- whoami
```

Show help:

```bash
supersudo --help
```
## Installation

```bash
cargo install --git https://github.com/cxnmai/supersudo.git
```

## Commands

```bash
supersudo setup
```

Interactive setup for config creation and optional sudo alias installation.

```bash
supersudo config init
supersudo config init --force
```

Create or overwrite the default user config at:

```text
${XDG_CONFIG_HOME:-~/.config}/supersudo/config.toml
```

```bash
supersudo path init
supersudo path remove
```

Add/remove this marked shell alias block:

```sh
# >>> supersudo alias >>>
alias sudo='supersudo'
# <<< supersudo alias <<<
```

Supported shell config targets:

- bash: `~/.bashrc`
- zsh: `~/.zshrc`
- unknown shell: `~/.profile`

Override with:

```bash
SUPERSUDO_SHELL_CONFIG=/path/to/rc supersudo path init
```

## Config loading

Config precedence:

1. `--config /path/to/config.toml`
2. `SUPERSUDO_CONFIG=/path/to/config.toml`
3. `$XDG_CONFIG_HOME/supersudo/config.toml`
4. `~/.config/supersudo/config.toml`
5. `/etc/supersudo/config.toml`
6. Built-in defaults

Normal command runs do not create config files automatically.

## Config example

See:

```text
examples/config.toml
examples/templates/password.txt
examples/templates/error.txt
examples/templates/success.txt
examples/animations/loading_bar.txt
```

## Templates

Templates support variables:

```text
{user}
{host}
{cwd}
{command}
{password}
{error}
{success}
{animation:name}
```

Padding/truncation:

```text
{command:pad=28}
{animation:loading_bar:pad=28}
{lit:Authentication required:pad=39}
```

Styles:

```text
{style:title}
{reset}
```

Example:

```toml
[styles]
title = "bold yellow"
value = "bright_white"
error = "bold bright_red"
```

## External template files

Long prompts can live in separate files:

```toml
[display]
template_file = "templates/password.txt"
error_template_file = "templates/error.txt"
success_template_file = "templates/success.txt"
```

Relative paths are resolved relative to the config file. External template/animation files larger than 1 MiB are rejected.

## Animations

Animations must be external files:

```toml
[animations]
loading_bar = "animations/loading_bar.txt"

[animation_speeds]
loading_bar = 80
```

Use in templates:

```text
{animation:loading_bar}
```

Animation file formats:

- one frame per line, or
- multi-line frames separated by a line containing `---`

## Development

```bash
cargo fmt
cargo check
cargo clippy -- -D warnings
cargo test
```
