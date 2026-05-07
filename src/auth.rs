use crossterm::cursor::{position, Hide, MoveTo, MoveToColumn, MoveUp, Show};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use zeroize::Zeroize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptState {
    Normal,
    Error,
    Success,
}

pub fn credentials_are_cached(real_sudo: &Path) -> Result<bool, String> {
    let status = Command::new(real_sudo)
        .arg("-n")
        .arg("-v")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|err| format!("failed to check sudo credential cache: {err}"))?;

    Ok(status.success())
}

pub fn authenticate_custom<F>(
    real_sudo: &Path,
    mut render_ui: F,
    feedback_char: char,
    attempts: u8,
    error_delay_ms: u64,
    success_delay_ms: u64,
) -> Result<(), String>
where
    F: FnMut(&str, PromptState, &str) -> Result<String, String>,
{
    let attempts = attempts.max(1);
    let mut message = String::new();
    let mut attempt = 1;

    let _tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|err| format!("failed to open /dev/tty for password input: {err}"))?;

    let mut password = Vec::new();
    let mut feedback = String::new();
    let start = reserve_redraw_region(&mut render_ui, &feedback, PromptState::Normal, &message)?;

    let mut stdout = io::stdout();
    execute!(stdout, Hide).map_err(|err| format!("failed to hide cursor: {err}"))?;

    let _guard = TerminalModeGuard::new()?;
    let mut error_until: Option<Instant> = None;
    redraw_ui(&mut render_ui, &feedback, prompt_state(error_until), &message, start)?;

    loop {
        let state = prompt_state(error_until);
        if state == PromptState::Normal && !message.is_empty() {
            message.clear();
            error_until = None;
            redraw_ui(&mut render_ui, &feedback, prompt_state(error_until), &message, start)?;
        }

        let timeout = error_until
            .and_then(|until| until.checked_duration_since(Instant::now()))
            .unwrap_or_else(|| Duration::from_millis(250));

        if !event::poll(timeout).map_err(|err| format!("failed to poll password input: {err}"))? {
            continue;
        }

        let event = event::read().map_err(|err| format!("failed to read password input: {err}"))?;
        let Event::Key(key) = event else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') | KeyCode::Char('d') => {
                    password.zeroize();
                    return Err("password input cancelled".to_string());
                }
                KeyCode::Char('z') => {
                    suspend_self()?;
                    redraw_ui(&mut render_ui, &feedback, prompt_state(error_until), &message, start)?;
                    continue;
                }
                KeyCode::Char('u') => {
                    password.zeroize();
                    password.clear();
                    feedback.clear();
                    redraw_ui(&mut render_ui, &feedback, prompt_state(error_until), &message, start)?;
                    continue;
                }
                KeyCode::Char('w') => {
                    pop_last_word(&mut password, &mut feedback);
                    redraw_ui(&mut render_ui, &feedback, prompt_state(error_until), &message, start)?;
                    continue;
                }
                _ => continue,
            }
        }

        match key.code {
            KeyCode::Enter => {
                let ok = validate_password(real_sudo, &password);
                password.zeroize();
                password.clear();
                feedback.clear();

                if ok? {
                    message = "Authentication successful".to_string();
                    redraw_ui(&mut render_ui, &feedback, PromptState::Success, &message, start)?;
                    if success_delay_ms > 0 {
                        thread::sleep(Duration::from_millis(success_delay_ms));
                    }
                    return Ok(());
                }

                if attempt >= attempts {
                    message = "Authentication failed".to_string();
                    redraw_ui(&mut render_ui, &feedback, PromptState::Error, &message, start)?;
                    return Err("authentication failed".to_string());
                }

                attempt += 1;
                message = "Authentication failed, try again".to_string();
                error_until = Some(Instant::now() + Duration::from_millis(error_delay_ms));
                redraw_ui(&mut render_ui, &feedback, PromptState::Error, &message, start)?;
            }
            KeyCode::Char(ch) => {
                if ch.is_control() {
                    continue;
                }
                let mut buf = [0; 4];
                password.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                feedback.push(feedback_char);
                redraw_ui(&mut render_ui, &feedback, prompt_state(error_until), &message, start)?;
            }
            KeyCode::Backspace => {
                if !password.is_empty() {
                    pop_last_utf8_char(&mut password);
                    feedback.pop();
                    redraw_ui(&mut render_ui, &feedback, prompt_state(error_until), &message, start)?;
                }
            }
            KeyCode::Esc => {
                password.zeroize();
                return Err("password input cancelled".to_string());
            }
            _ => {}
        }
    }
}

fn prompt_state(error_until: Option<Instant>) -> PromptState {
    match error_until {
        Some(until) if Instant::now() < until => PromptState::Error,
        _ => PromptState::Normal,
    }
}

fn reserve_redraw_region<F>(
    render_ui: &mut F,
    feedback: &str,
    state: PromptState,
    message: &str,
) -> Result<(u16, u16), String>
where
    F: FnMut(&str, PromptState, &str) -> Result<String, String>,
{
    let rendered = render_ui(feedback, state, message)?;
    let lines = rendered_line_count(&rendered).max(1);
    let mut stdout = io::stdout();

    for _ in 0..lines {
        stdout
            .write_all(b"\n")
            .map_err(|err| format!("failed to reserve password UI space: {err}"))?;
    }
    stdout
        .flush()
        .map_err(|err| format!("failed to flush password UI reservation: {err}"))?;

    execute!(stdout, MoveUp(lines as u16), MoveToColumn(0))
        .map_err(|err| format!("failed to move to password UI start: {err}"))?;

    position().map_err(|err| format!("failed to get password UI start position: {err}"))
}

fn redraw_ui<F>(
    render_ui: &mut F,
    feedback: &str,
    state: PromptState,
    message: &str,
    start: (u16, u16),
) -> Result<(), String>
where
    F: FnMut(&str, PromptState, &str) -> Result<String, String>,
{
    let rendered = render_ui(feedback, state, message)?;
    let mut stdout = io::stdout();

    execute!(stdout, MoveTo(start.0, start.1), Clear(ClearType::FromCursorDown))
        .map_err(|err| format!("failed to prepare password UI redraw: {err}"))?;

    let rendered = rendered.replace('\n', "\r\n");
    stdout
        .write_all(rendered.as_bytes())
        .and_then(|_| stdout.flush())
        .map_err(|err| format!("failed to write password UI: {err}"))?;

    Ok(())
}

fn suspend_self() -> Result<(), String> {
    disable_raw_mode().map_err(|err| format!("failed to disable raw mode before suspend: {err}"))?;
    execute!(io::stdout(), Show).map_err(|err| format!("failed to show cursor before suspend: {err}"))?;

    let rc = unsafe { libc::raise(libc::SIGTSTP) };
    if rc != 0 {
        return Err("failed to suspend process".to_string());
    }

    enable_raw_mode().map_err(|err| format!("failed to re-enable raw mode after resume: {err}"))?;
    execute!(io::stdout(), Hide).map_err(|err| format!("failed to hide cursor after resume: {err}"))?;
    Ok(())
}

fn rendered_line_count(rendered: &str) -> usize {
    let newline_count = rendered.matches('\n').count();
    if rendered.ends_with('\n') {
        newline_count
    } else {
        newline_count + 1
    }
}

fn validate_password(real_sudo: &Path, password: &[u8]) -> Result<bool, String> {
    let mut child = Command::new(real_sudo)
        .arg("-S")
        .arg("-p")
        .arg("")
        .arg("-v")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("failed to start sudo password validation: {err}"))?;

    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "failed to open sudo validation stdin".to_string())?;
        stdin
            .write_all(password)
            .and_then(|_| stdin.write_all(b"\n"))
            .and_then(|_| stdin.flush())
            .map_err(|err| format!("failed to write password to sudo: {err}"))?;
    }

    let status = child
        .wait()
        .map_err(|err| format!("failed waiting for sudo validation: {err}"))?;

    Ok(status.success())
}

fn pop_last_word(password: &mut Vec<u8>, feedback: &mut String) {
    while !password.is_empty() && password.last().is_some_and(|b| b.is_ascii_whitespace()) {
        pop_last_utf8_char(password);
        feedback.pop();
    }

    while !password.is_empty() && password.last().is_some_and(|b| !b.is_ascii_whitespace()) {
        pop_last_utf8_char(password);
        feedback.pop();
    }
}

fn pop_last_utf8_char(bytes: &mut Vec<u8>) {
    while let Some(byte) = bytes.pop() {
        if byte & 0b1100_0000 != 0b1000_0000 {
            break;
        }
    }
}

struct TerminalModeGuard;

impl TerminalModeGuard {
    fn new() -> Result<Self, String> {
        enable_raw_mode().map_err(|err| format!("failed to enable raw terminal mode: {err}"))?;
        Ok(Self)
    }
}

impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), Show);
    }
}
