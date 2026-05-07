use crossterm::cursor::{Hide, MoveTo, MoveToColumn, MoveUp, Show, position};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode};
use secrets::SecretBox;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const MAX_PASSWORD_BYTES: usize = 1024;

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
    F: FnMut(&str, PromptState, &str, u128) -> Result<String, String>,
{
    let animation_start = Instant::now();
    let attempts = attempts.max(1);
    let mut message = String::new();
    let mut attempt = 1;

    let _tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|err| format!("failed to open /dev/tty for password input: {err}"))?;

    let mut password = PasswordBuffer::new();
    let mut feedback = String::new();
    let start = reserve_redraw_region(
        &mut render_ui,
        &feedback,
        PromptState::Normal,
        &message,
        animation_start,
    )?;

    let mut stdout = io::stdout();
    execute!(stdout, Hide).map_err(|err| format!("failed to hide cursor: {err}"))?;

    let _guard = TerminalModeGuard::new()?;
    let mut error_until: Option<Instant> = None;
    redraw_ui(
        &mut render_ui,
        &feedback,
        prompt_state(error_until),
        &message,
        start,
        animation_start,
    )?;

    loop {
        let state = prompt_state(error_until);
        if state == PromptState::Normal && !message.is_empty() {
            message.clear();
            error_until = None;
            redraw_ui(
                &mut render_ui,
                &feedback,
                prompt_state(error_until),
                &message,
                start,
                animation_start,
            )?;
        }

        let timeout = error_until
            .and_then(|until| until.checked_duration_since(Instant::now()))
            .unwrap_or_else(|| Duration::from_millis(80));

        if !event::poll(timeout).map_err(|err| format!("failed to poll password input: {err}"))? {
            redraw_ui(
                &mut render_ui,
                &feedback,
                prompt_state(error_until),
                &message,
                start,
                animation_start,
            )?;
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
                    password.clear();
                    return Err("password input cancelled".to_string());
                }
                KeyCode::Char('z') => {
                    suspend_self()?;
                    redraw_ui(
                        &mut render_ui,
                        &feedback,
                        prompt_state(error_until),
                        &message,
                        start,
                        animation_start,
                    )?;
                    continue;
                }
                KeyCode::Char('u') => {
                    password.clear();
                    feedback.clear();
                    redraw_ui(
                        &mut render_ui,
                        &feedback,
                        prompt_state(error_until),
                        &message,
                        start,
                        animation_start,
                    )?;
                    continue;
                }
                KeyCode::Char('w') => {
                    let removed = password.pop_word();
                    for _ in 0..removed {
                        feedback.pop();
                    }
                    redraw_ui(
                        &mut render_ui,
                        &feedback,
                        prompt_state(error_until),
                        &message,
                        start,
                        animation_start,
                    )?;
                    continue;
                }
                _ => continue,
            }
        }

        match key.code {
            KeyCode::Enter => {
                let ok = password.with_bytes(|bytes| validate_password(real_sudo, bytes));
                password.clear();
                feedback.clear();

                if ok? {
                    message = "Authentication successful".to_string();
                    redraw_ui(
                        &mut render_ui,
                        &feedback,
                        PromptState::Success,
                        &message,
                        start,
                        animation_start,
                    )?;
                    if success_delay_ms > 0 {
                        thread::sleep(Duration::from_millis(success_delay_ms));
                    }
                    return Ok(());
                }

                if attempt >= attempts {
                    message = "Authentication failed".to_string();
                    redraw_ui(
                        &mut render_ui,
                        &feedback,
                        PromptState::Error,
                        &message,
                        start,
                        animation_start,
                    )?;
                    return Err("authentication failed".to_string());
                }

                attempt += 1;
                message = "Authentication failed, try again".to_string();
                error_until = Some(Instant::now() + Duration::from_millis(error_delay_ms));
                redraw_ui(
                    &mut render_ui,
                    &feedback,
                    PromptState::Error,
                    &message,
                    start,
                    animation_start,
                )?;
            }
            KeyCode::Char(ch) => {
                if ch.is_control() {
                    continue;
                }
                password.push_char(ch)?;
                feedback.push(feedback_char);
                redraw_ui(
                    &mut render_ui,
                    &feedback,
                    prompt_state(error_until),
                    &message,
                    start,
                    animation_start,
                )?;
            }
            KeyCode::Backspace if !password.is_empty() => {
                password.pop_char();
                feedback.pop();
                redraw_ui(
                    &mut render_ui,
                    &feedback,
                    prompt_state(error_until),
                    &message,
                    start,
                    animation_start,
                )?;
            }
            KeyCode::Esc => {
                password.clear();
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
    animation_start: Instant,
) -> Result<(u16, u16), String>
where
    F: FnMut(&str, PromptState, &str, u128) -> Result<String, String>,
{
    let rendered = render_ui(
        feedback,
        state,
        message,
        animation_start.elapsed().as_millis(),
    )?;
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
    animation_start: Instant,
) -> Result<(), String>
where
    F: FnMut(&str, PromptState, &str, u128) -> Result<String, String>,
{
    let rendered = render_ui(
        feedback,
        state,
        message,
        animation_start.elapsed().as_millis(),
    )?;
    let mut stdout = io::stdout();

    execute!(
        stdout,
        MoveTo(start.0, start.1),
        Clear(ClearType::FromCursorDown)
    )
    .map_err(|err| format!("failed to prepare password UI redraw: {err}"))?;

    let rendered = rendered.replace('\n', "\r\n");
    stdout
        .write_all(rendered.as_bytes())
        .and_then(|_| stdout.flush())
        .map_err(|err| format!("failed to write password UI: {err}"))?;

    Ok(())
}

fn suspend_self() -> Result<(), String> {
    disable_raw_mode()
        .map_err(|err| format!("failed to disable raw mode before suspend: {err}"))?;
    execute!(io::stdout(), Show)
        .map_err(|err| format!("failed to show cursor before suspend: {err}"))?;

    let rc = unsafe { libc::raise(libc::SIGTSTP) };
    if rc != 0 {
        return Err("failed to suspend process".to_string());
    }

    enable_raw_mode().map_err(|err| format!("failed to re-enable raw mode after resume: {err}"))?;
    execute!(io::stdout(), Hide)
        .map_err(|err| format!("failed to hide cursor after resume: {err}"))?;
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

struct PasswordBuffer {
    secret: SecretBox<[u8; MAX_PASSWORD_BYTES]>,
    len: usize,
}

impl PasswordBuffer {
    fn new() -> Self {
        Self {
            secret: SecretBox::zero(),
            len: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn push_char(&mut self, ch: char) -> Result<(), String> {
        let mut encoded = [0; 4];
        let encoded = ch.encode_utf8(&mut encoded).as_bytes();
        let end = self
            .len
            .checked_add(encoded.len())
            .ok_or_else(|| "password is too long".to_string())?;

        if end > MAX_PASSWORD_BYTES {
            return Err(format!(
                "password is too long; maximum is {MAX_PASSWORD_BYTES} bytes"
            ));
        }

        {
            let mut secret = self.secret.borrow_mut();
            secret[self.len..end].copy_from_slice(encoded);
        }
        self.len = end;
        Ok(())
    }

    fn pop_char(&mut self) -> bool {
        let Some(start) = self.last_char_start() else {
            return false;
        };
        self.zero_range(start, self.len);
        self.len = start;
        true
    }

    fn pop_word(&mut self) -> usize {
        let mut removed = 0;

        while self
            .last_byte()
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            if self.pop_char() {
                removed += 1;
            }
        }

        while self
            .last_byte()
            .is_some_and(|byte| !byte.is_ascii_whitespace())
        {
            if self.pop_char() {
                removed += 1;
            }
        }

        removed
    }

    fn clear(&mut self) {
        self.zero_range(0, self.len);
        self.len = 0;
    }

    fn with_bytes<T>(&self, f: impl FnOnce(&[u8]) -> T) -> T {
        let secret = self.secret.borrow();
        f(&secret[..self.len])
    }

    fn last_byte(&self) -> Option<u8> {
        if self.len == 0 {
            return None;
        }

        self.with_bytes(|bytes| bytes.last().copied())
    }

    fn last_char_start(&self) -> Option<usize> {
        if self.len == 0 {
            return None;
        }

        self.with_bytes(|bytes| {
            let mut start = bytes.len() - 1;
            while start > 0 && bytes[start] & 0b1100_0000 == 0b1000_0000 {
                start -= 1;
            }
            Some(start)
        })
    }

    fn zero_range(&mut self, start: usize, end: usize) {
        if start >= end {
            return;
        }

        let mut secret = self.secret.borrow_mut();
        secret[start..end].fill(0);
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_PASSWORD_BYTES, PasswordBuffer};

    #[test]
    fn password_buffer_appends_ascii() {
        let mut password = PasswordBuffer::new();

        password.push_char('a').unwrap();
        password.push_char('b').unwrap();

        password.with_bytes(|bytes| assert_eq!(bytes, b"ab"));
    }

    #[test]
    fn password_buffer_appends_multibyte_utf8() {
        let mut password = PasswordBuffer::new();

        password.push_char('a').unwrap();
        password.push_char('é').unwrap();

        password.with_bytes(|bytes| assert_eq!(bytes, "aé".as_bytes()));
    }

    #[test]
    fn password_buffer_backspace_removes_one_utf8_char() {
        let mut password = PasswordBuffer::new();

        password.push_char('a').unwrap();
        password.push_char('é').unwrap();
        assert!(password.pop_char());

        password.with_bytes(|bytes| assert_eq!(bytes, b"a"));
    }

    #[test]
    fn password_buffer_pop_word_removes_trailing_word_and_whitespace() {
        let mut password = PasswordBuffer::new();

        for ch in "alpha beta  ".chars() {
            password.push_char(ch).unwrap();
        }

        assert_eq!(password.pop_word(), 6);
        password.with_bytes(|bytes| assert_eq!(bytes, b"alpha "));
    }

    #[test]
    fn password_buffer_clear_removes_visible_contents() {
        let mut password = PasswordBuffer::new();

        password.push_char('x').unwrap();
        password.clear();

        assert!(password.is_empty());
        password.with_bytes(|bytes| assert!(bytes.is_empty()));
    }

    #[test]
    fn password_buffer_rejects_overflow() {
        let mut password = PasswordBuffer::new();

        for _ in 0..MAX_PASSWORD_BYTES {
            password.push_char('x').unwrap();
        }

        assert!(password.push_char('x').is_err());
        password.with_bytes(|bytes| assert_eq!(bytes.len(), MAX_PASSWORD_BYTES));
    }
}
