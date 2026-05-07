use crossterm::cursor::{position, Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use zeroize::Zeroize;

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
) -> Result<(), String>
where
    F: FnMut(&str) -> Result<String, String>,
{
    let attempts = attempts.max(1);

    for attempt in 1..=attempts {
        let mut password = read_password_with_feedback(&mut render_ui, feedback_char)?;
        let ok = validate_password(real_sudo, &password);
        password.zeroize();

        match ok? {
            true => return Ok(()),
            false if attempt < attempts => eprintln!("\nsupersudo: authentication failed, try again"),
            false => return Err("authentication failed".to_string()),
        }
    }

    Err("authentication failed".to_string())
}

fn read_password_with_feedback<F>(render_ui: &mut F, feedback_char: char) -> Result<Vec<u8>, String>
where
    F: FnMut(&str) -> Result<String, String>,
{
    let _tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|err| format!("failed to open /dev/tty for password input: {err}"))?;

    let start = position().map_err(|err| format!("failed to get cursor position: {err}"))?;
    let mut stdout = io::stdout();
    execute!(stdout, Hide).map_err(|err| format!("failed to hide cursor: {err}"))?;

    let _guard = TerminalModeGuard::new()?;
    let mut password = Vec::new();
    let mut feedback = String::new();
    redraw_ui(render_ui, &feedback, start)?;

    loop {
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
                    redraw_ui(render_ui, &feedback, start)?;
                    continue;
                }
                KeyCode::Char('u') => {
                    password.zeroize();
                    password.clear();
                    feedback.clear();
                    redraw_ui(render_ui, &feedback, start)?;
                    continue;
                }
                KeyCode::Char('w') => {
                    pop_last_word(&mut password, &mut feedback);
                    redraw_ui(render_ui, &feedback, start)?;
                    continue;
                }
                _ => continue,
            }
        }

        match key.code {
            KeyCode::Enter => {
                redraw_ui(render_ui, &feedback, start)?;
                print!("\r\n");
                io::stdout()
                    .flush()
                    .map_err(|err| format!("failed to flush password newline: {err}"))?;
                break;
            }
            KeyCode::Char(ch) => {
                if ch.is_control() {
                    continue;
                }
                let mut buf = [0; 4];
                password.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                feedback.push(feedback_char);
                redraw_ui(render_ui, &feedback, start)?;
            }
            KeyCode::Backspace => {
                if !password.is_empty() {
                    pop_last_utf8_char(&mut password);
                    feedback.pop();
                    redraw_ui(render_ui, &feedback, start)?;
                }
            }
            KeyCode::Esc => {
                password.zeroize();
                return Err("password input cancelled".to_string());
            }
            _ => {}
        }
    }

    Ok(password)
}

fn redraw_ui<F>(render_ui: &mut F, feedback: &str, start: (u16, u16)) -> Result<(), String>
where
    F: FnMut(&str) -> Result<String, String>,
{
    let rendered = render_ui(feedback)?;
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
