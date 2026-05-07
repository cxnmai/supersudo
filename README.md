# supersudo

`supersudo` is an experimental Rust wrapper around the user's existing `sudo` installation. It provides configurable terminal UI, live password feedback, prompt states, and animations while delegating authentication, sudoers policy, credential caching, and command execution to the system `sudo` binary.

<img width="1048" height="968" alt="githubgif1" src="https://github.com/user-attachments/assets/1cb40070-ef5d-4421-a375-b0d8a0a514e1" />


<img width="1076" height="900" alt="githubgif2" src="https://github.com/user-attachments/assets/1ff6bb13-bf9d-400b-ac40-672af860287b" />


## Security model

`supersudo` does **not** bundle or replace sudo.

There are two input modes:

```toml
[input]
mode = "sudo"
```

Real sudo reads the password. This is the safest mode, but live password feedback/animations during typing are not available.

```toml
[input]
mode = "custom"
```

`supersudo` reads the password to render live feedback and animated UI, then validates it with:

```bash
/usr/bin/sudo -S -p "" -v
```

If validation succeeds, it runs the requested command with:

```bash
/usr/bin/sudo -n ...
```

In custom mode, the password is never passed through args, env vars, config files, temp files, or shell commands. It is sent only through sudo stdin and zeroized after validation/cancellation.

During custom password entry, `supersudo` stores the password in a fixed-size protected memory allocation using the Rust `secrets` crate rather than a growable heap buffer. This avoids heap reallocations that could leave stale password prefixes behind. The protected allocation is locked with `mlock(2)`, guarded, inaccessible outside explicit borrow scopes with `mprotect(2)`, and zeroed when released on supported Unix targets.

This is still best-effort userspace protection. Custom mode necessarily exposes the password to the `supersudo` process while typing and validating it, and cannot eliminate exposure through terminal/kernel buffers, sudo stdin, root/debugger access, process compromise, or abnormal termination such as `SIGKILL`.

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

## Caveats

- `mode = "custom"` is security-sensitive because `supersudo` sees the password.
- Terminal state is restored on normal exits, but no program can recover from `SIGKILL`.
- Do not use untrusted configs/templates; they can display arbitrary terminal text.
