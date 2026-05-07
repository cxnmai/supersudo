# supersudo

`supersudo` is an experimental Rust wrapper around the user's existing `sudo` installation.

The goal is to eventually provide richer sudo prompt customization while still delegating authentication, policy, updates, and command execution to the system-provided `sudo` binary.

## Current status

Implemented so far:

- Transparent wrapping of the real `sudo`
- Config file loading from several locations
- Configurable real sudo path
- Recursion protection so `supersudo` does not accidentally call itself
- Example config for testing

Prompt customization and UI features are planned but not implemented yet.

## How wrapping works

At the moment, `supersudo` simply forwards arguments to the real sudo binary.

For example:

```bash
supersudo apt update
```

executes roughly as:

```bash
/usr/bin/sudo apt update
```

The wrapper uses Unix `exec`, meaning the `supersudo` process is replaced by the real `sudo` process instead of staying around as a parent process.

## Real sudo resolution

`supersudo` does **not** bundle sudo.

It resolves the real sudo path using this order:

1. Config file:

   ```toml
   [general]
   real_sudo = "/usr/bin/sudo"
   ```

2. Environment variable:

   ```bash
   SUPERSUDO_REAL_SUDO=/usr/bin/sudo supersudo apt update
   ```

3. Built-in candidates:

   ```text
   /usr/bin/sudo
   /bin/sudo
   /usr/local/bin/sudo
   ```

The selected path must be an absolute file path. `supersudo` also checks that the selected sudo path does not point back to the current `supersudo` executable.

## Configuration

Config files are TOML.

Config precedence, from highest to lowest:

1. CLI flag:

   ```bash
   supersudo --config /path/to/config.toml -- apt update
   ```

2. Environment variable:

   ```bash
   SUPERSUDO_CONFIG=/path/to/config.toml supersudo apt update
   ```

3. User config via XDG:

   ```text
   $XDG_CONFIG_HOME/supersudo/config.toml
   ```

4. User config fallback:

   ```text
   ~/.config/supersudo/config.toml
   ```

5. System config:

   ```text
   /etc/supersudo/config.toml
   ```

6. Built-in defaults

If no config file exists, no file or directory is created automatically. Built-in defaults are used instead.

## Current config schema

```toml
[general]
real_sudo = "/usr/bin/sudo"

[prompt]
template = "Password: "

[ui]
enabled = true
```

Currently, only this field is actively used:

```toml
[general]
real_sudo = "/usr/bin/sudo"
```

The `[prompt]` and `[ui]` sections exist as placeholders for upcoming customization work.

## Test config

A test config lives at:

```text
examples/config.toml
```

You can test config loading with:

```bash
cargo run -- --config examples/config.toml -- -V
```

This should execute the real sudo with `-V`.

## Development

Check the project with:

```bash
cargo check
```

Run it with:

```bash
cargo run -- <sudo args>
```

Example:

```bash
cargo run -- -V
```

## Planned next steps

Possible next features:

- Render a custom sudo prompt from the `[prompt]` template
- Add variables such as `{user}`, `{host}`, `{cwd}`, and `{command}`
- Carefully handle existing sudo prompt flags like `-p` / `--prompt`
- Add shell integration so users can type `sudo` but route through `supersudo`
- Add an init command to create `~/.config/supersudo/config.toml`
- Add optional pre-prompt terminal UI or animations
