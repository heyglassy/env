use std::{
    cell::RefCell,
    env,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    process::{self, Command, Output, Stdio},
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use chrono::Local;
use crossterm::{
    cursor::{self, SetCursorStyle},
    event::{
        self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, KeyboardEnhancementFlags, MediaKeyCode, ModifierKeyCode, MouseButton,
        MouseEvent, MouseEventKind, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{self, Clear, ClearType},
};
use libghostty_vt::{
    Terminal, TerminalOptions, focus as ghostty_focus, key as ghostty_key, mouse as ghostty_mouse,
    render::{CellIterator, CursorVisualStyle, RenderState, RowIterator},
    style::{PaletteIndex, RgbColor},
    terminal::Mode,
};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use ratatui::{
    Terminal as RatatuiTerminal,
    backend::CrosstermBackend,
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};
use tracing::{debug, info, warn};
use tracing_appender::non_blocking::WorkerGuard;

const FRAME_TIME: Duration = Duration::from_millis(16);
const EVENT_POLL: Duration = Duration::from_millis(33);
const GHOSTTY_CONFIG_TIMEOUT: Duration = Duration::from_millis(250);
const MACOS_APPEARANCE_TIMEOUT: Duration = Duration::from_millis(100);
const COLOR_QUERY_TIMEOUT: Duration = Duration::from_millis(120);
const COLOR_QUERY_QUIET: Duration = Duration::from_millis(10);
const ANSI_PALETTE_QUERY_COUNT: usize = 16;
static TERMINAL_RESTORED: AtomicBool = AtomicBool::new(true);

fn main() -> Result<()> {
    let _trace_guard = init_tracing();
    let command = command_from_args();
    info!(?command, "starting insignia");
    let mut app = TerminalApp::new(command)?;
    app.run()
}

fn command_from_args() -> Vec<String> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|arg| arg == "--") {
        args.remove(0);
    }

    if args.is_empty() {
        vec![env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())]
    } else {
        args
    }
}

fn init_tracing() -> Option<WorkerGuard> {
    let enabled = env::var("INSIGNIA_TRACE")
        .ok()
        .is_some_and(|value| !matches!(value.as_str(), "" | "0" | "false" | "off"));
    if !enabled {
        return None;
    }

    let log_dir = env::var_os("INSIGNIA_TRACE_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Logs/insignia"))
        })
        .unwrap_or_else(|| env::temp_dir().join("insignia"));

    if let Err(error) = fs::create_dir_all(&log_dir) {
        eprintln!(
            "insignia: failed to create trace log directory {}: {error}",
            log_dir.display()
        );
        return None;
    }

    let appender = tracing_appender::rolling::never(&log_dir, "theme.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let subscriber = tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(true)
        .with_level(true)
        .with_max_level(tracing::Level::DEBUG)
        .finish();

    if let Err(error) = tracing::subscriber::set_global_default(subscriber) {
        eprintln!("insignia: failed to initialize tracing: {error}");
        return None;
    }

    info!(
        version = env!("CARGO_PKG_VERSION"),
        pid = process::id(),
        log_dir = %log_dir.display(),
        "tracing initialized"
    );
    Some(guard)
}

struct TerminalApp {
    command: Vec<String>,
    status: StatusBar,
    overlay: InteractionOverlay,
    theme: Theme,
    input_tracker: ShellInputTracker,
}

impl TerminalApp {
    fn new(command: Vec<String>) -> Result<Self> {
        Ok(Self {
            command,
            status: StatusBar::new()?,
            overlay: InteractionOverlay::default(),
            theme: Theme::default(),
            input_tracker: ShellInputTracker::default(),
        })
    }

    fn run(&mut self) -> Result<()> {
        let _guard = TerminalGuard::enter()?;
        self.theme = match Theme::query() {
            Ok(theme) => theme,
            Err(error) => {
                warn!(%error, "theme query failed; using default theme");
                Theme::default()
            }
        };
        debug!(
            foreground = %format_rgb(self.theme.foreground),
            background = %format_rgb(self.theme.background),
            cursor = %format_rgb(self.theme.cursor_color),
            selection_foreground = %format_rgb(self.theme.selection_foreground),
            selection_background = %format_rgb(self.theme.selection_background),
            theme_setting = ?self.theme.theme_setting,
            "theme selected"
        );
        self.status.set_theme(self.theme.status_style());
        self.overlay.set_theme(self.theme.interaction_style());

        let mut stdout = io::stdout();
        execute!(
            stdout,
            Clear(ClearType::All),
            cursor::MoveTo(0, 0),
            EnableMouseCapture,
            EnableFocusChange,
            EnableBracketedPaste,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                    | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
                    | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
            ),
            cursor::Hide
        )?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = RatatuiTerminal::new(backend)?;
        let initial = terminal.size()?;
        let initial_pty_rows = pty_rows(initial.height);
        let mut session = PtySession::spawn(&self.command, initial.width, initial_pty_rows)?;

        let mut vt = Box::new(Terminal::new(TerminalOptions {
            cols: initial.width,
            rows: initial_pty_rows,
            max_scrollback: 10_000,
        })?);
        self.theme.apply_to_vt(&mut vt)?;
        let status_cwd = Rc::clone(&self.status.cwd);
        vt.on_pwd_changed(move |term| {
            if let Ok(pwd) = term.pwd()
                && !pwd.is_empty()
            {
                notify_outer_terminal_pwd(pwd);
                *status_cwd.borrow_mut() = display_pwd(pwd);
            }
        })?;
        vt.on_pty_write({
            let writer = Rc::clone(&session.writer);
            move |_, bytes| {
                let mut writer = writer.borrow_mut();
                let _ = writer.write_all(bytes);
                let _ = writer.flush();
            }
        })?;
        let mut render_state = RenderState::new()?;
        let mut renderer = GhosttyRenderer;
        let mut key_encoder = ghostty_key::Encoder::new()?;
        let mut mouse_encoder = ghostty_mouse::Encoder::new()?;
        let mut last_frame = Instant::now()
            .checked_sub(FRAME_TIME)
            .unwrap_or_else(Instant::now);
        let mut dirty = true;
        let mut running = true;

        while running {
            while let Ok(bytes) = session.output_rx.try_recv() {
                vt.vt_write(&bytes);
                dirty = true;
            }

            if dirty {
                let now = Instant::now();
                if now.duration_since(last_frame) >= FRAME_TIME {
                    terminal.draw(|frame| {
                        let area = frame.area();
                        let body = terminal_area(area);
                        renderer.render(
                            &mut vt,
                            &mut render_state,
                            body,
                            frame.buffer_mut(),
                            self.theme.cursor_text,
                        );
                        self.overlay.render(body, frame.buffer_mut());
                        self.status.render(area, frame.buffer_mut());
                    })?;
                    last_frame = now;
                    dirty = false;
                }
            }

            let poll_timeout = if dirty {
                FRAME_TIME.saturating_sub(last_frame.elapsed())
            } else {
                EVENT_POLL
            };

            if event::poll(poll_timeout)? {
                match event::read()? {
                    Event::Key(key) => {
                        if should_quit(key) {
                            break;
                        }

                        if self.input_tracker.handle_key(key, &self.status) {
                            dirty = true;
                        }

                        let key_cleared_overlay = key_resets_overlay(key) && self.overlay.clear();
                        if key_cleared_overlay {
                            dirty = true;
                        }

                        if key.kind == KeyEventKind::Press
                            && key.code == KeyCode::Esc
                            && key_cleared_overlay
                        {
                            continue;
                        }

                        if let Some(encoded) = encode_key_event(&mut key_encoder, &vt, key)? {
                            session.write_all(&encoded)?;
                        }
                    }
                    Event::Mouse(mouse) => {
                        let area = terminal_area(terminal.size()?.into());
                        if vt.is_mouse_tracking().unwrap_or(false) {
                            if let Some(encoded) =
                                encode_mouse_event(&mut mouse_encoder, &vt, mouse, area)?
                            {
                                session.write_all(&encoded)?;
                            }
                        } else if self.overlay.handle_mouse(mouse, area) {
                            dirty = true;
                        }
                    }
                    Event::Paste(text) => session.write_all(text.as_bytes())?,
                    Event::FocusGained => {
                        if vt.mode(Mode::FOCUS_EVENT).unwrap_or(false) {
                            session
                                .write_all(&encode_focus_event(ghostty_focus::Event::Gained)?)?;
                        }
                    }
                    Event::FocusLost => {
                        if vt.mode(Mode::FOCUS_EVENT).unwrap_or(false) {
                            session.write_all(&encode_focus_event(ghostty_focus::Event::Lost)?)?;
                        }
                    }
                    Event::Resize(cols, rows) => {
                        let rows = pty_rows(rows);
                        session.resize(cols, rows)?;
                        vt.resize(cols, rows, 0, 0)?;
                        self.overlay.constrain_to(Rect::new(0, 0, cols, rows));
                        dirty = true;
                    }
                }
            }

            if !session.child_is_alive() {
                running = false;
            }
        }

        Ok(())
    }
}

fn pty_rows(total_rows: u16) -> u16 {
    total_rows.saturating_sub(1).max(1)
}

fn terminal_area(area: Rect) -> Rect {
    Rect {
        height: area.height.saturating_sub(1),
        ..area
    }
}

fn should_quit(key: KeyEvent) -> bool {
    key.kind == KeyEventKind::Press
        && key.code == KeyCode::Char('q')
        && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn key_resets_overlay(key: KeyEvent) -> bool {
    key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat
}

fn encode_key_event(
    encoder: &mut ghostty_key::Encoder<'_>,
    vt: &Terminal<'_, '_>,
    key: KeyEvent,
) -> Result<Option<Vec<u8>>> {
    let Some(ghostty_key) = crossterm_key_to_ghostty(key.code) else {
        return Ok(None);
    };
    let mut event = ghostty_key::Event::new()?;
    event
        .set_action(crossterm_key_action(key.kind))
        .set_key(ghostty_key)
        .set_mods(crossterm_key_mods(key.modifiers))
        .set_utf8(key_text(key));

    if let KeyCode::Char(c) = key.code {
        event.set_unshifted_codepoint(unshifted_char(c));
    }

    let mut encoded = Vec::with_capacity(32);
    encoder
        .set_options_from_terminal(vt)
        .set_macos_option_as_alt(ghostty_key::OptionAsAlt::True)
        .encode_to_vec(&event, &mut encoded)?;
    Ok((!encoded.is_empty()).then_some(encoded))
}

fn crossterm_key_action(kind: KeyEventKind) -> ghostty_key::Action {
    match kind {
        KeyEventKind::Release => ghostty_key::Action::Release,
        KeyEventKind::Repeat => ghostty_key::Action::Repeat,
        KeyEventKind::Press => ghostty_key::Action::Press,
    }
}

fn crossterm_key_mods(modifiers: KeyModifiers) -> ghostty_key::Mods {
    let mut mods = ghostty_key::Mods::empty();
    if modifiers.contains(KeyModifiers::SHIFT) {
        mods |= ghostty_key::Mods::SHIFT;
    }
    if modifiers.intersects(KeyModifiers::ALT | KeyModifiers::META) {
        mods |= ghostty_key::Mods::ALT;
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        mods |= ghostty_key::Mods::CTRL;
    }
    if modifiers.contains(KeyModifiers::SUPER) {
        mods |= ghostty_key::Mods::SUPER;
    }
    mods
}

fn key_text(key: KeyEvent) -> Option<String> {
    let KeyCode::Char(c) = key.code else {
        return None;
    };
    if key.modifiers.contains(KeyModifiers::CONTROL) || c.is_control() {
        None
    } else {
        Some(c.to_string())
    }
}

fn unshifted_char(c: char) -> char {
    match c {
        'A'..='Z' => c.to_ascii_lowercase(),
        ')' => '0',
        '!' => '1',
        '@' => '2',
        '#' => '3',
        '$' => '4',
        '%' => '5',
        '^' => '6',
        '&' => '7',
        '*' => '8',
        '(' => '9',
        '_' => '-',
        '+' => '=',
        '{' => '[',
        '}' => ']',
        '|' => '\\',
        ':' => ';',
        '"' => '\'',
        '<' => ',',
        '>' => '.',
        '?' => '/',
        '~' => '`',
        _ => c,
    }
}

fn crossterm_key_to_ghostty(code: KeyCode) -> Option<ghostty_key::Key> {
    let key = match code {
        KeyCode::Backspace => ghostty_key::Key::Backspace,
        KeyCode::Enter => ghostty_key::Key::Enter,
        KeyCode::Left => ghostty_key::Key::ArrowLeft,
        KeyCode::Right => ghostty_key::Key::ArrowRight,
        KeyCode::Up => ghostty_key::Key::ArrowUp,
        KeyCode::Down => ghostty_key::Key::ArrowDown,
        KeyCode::Home => ghostty_key::Key::Home,
        KeyCode::End => ghostty_key::Key::End,
        KeyCode::PageUp => ghostty_key::Key::PageUp,
        KeyCode::PageDown => ghostty_key::Key::PageDown,
        KeyCode::Tab | KeyCode::BackTab => ghostty_key::Key::Tab,
        KeyCode::Delete => ghostty_key::Key::Delete,
        KeyCode::Insert => ghostty_key::Key::Insert,
        KeyCode::Esc => ghostty_key::Key::Escape,
        KeyCode::CapsLock => ghostty_key::Key::CapsLock,
        KeyCode::ScrollLock => ghostty_key::Key::ScrollLock,
        KeyCode::NumLock => ghostty_key::Key::NumLock,
        KeyCode::PrintScreen => ghostty_key::Key::PrintScreen,
        KeyCode::Pause => ghostty_key::Key::Pause,
        KeyCode::Menu => ghostty_key::Key::ContextMenu,
        KeyCode::KeypadBegin => ghostty_key::Key::NumpadBegin,
        KeyCode::F(n) => function_key(n)?,
        KeyCode::Char(c) => character_key(c),
        KeyCode::Media(media) => media_key(media)?,
        KeyCode::Modifier(modifier) => modifier_key(modifier)?,
        KeyCode::Null => ghostty_key::Key::Unidentified,
    };
    Some(key)
}

fn character_key(c: char) -> ghostty_key::Key {
    match unshifted_char(c) {
        '`' => ghostty_key::Key::Backquote,
        '\\' => ghostty_key::Key::Backslash,
        '[' => ghostty_key::Key::BracketLeft,
        ']' => ghostty_key::Key::BracketRight,
        ',' => ghostty_key::Key::Comma,
        '0' => ghostty_key::Key::Digit0,
        '1' => ghostty_key::Key::Digit1,
        '2' => ghostty_key::Key::Digit2,
        '3' => ghostty_key::Key::Digit3,
        '4' => ghostty_key::Key::Digit4,
        '5' => ghostty_key::Key::Digit5,
        '6' => ghostty_key::Key::Digit6,
        '7' => ghostty_key::Key::Digit7,
        '8' => ghostty_key::Key::Digit8,
        '9' => ghostty_key::Key::Digit9,
        '=' => ghostty_key::Key::Equal,
        'a' | 'A' => ghostty_key::Key::A,
        'b' | 'B' => ghostty_key::Key::B,
        'c' | 'C' => ghostty_key::Key::C,
        'd' | 'D' => ghostty_key::Key::D,
        'e' | 'E' => ghostty_key::Key::E,
        'f' | 'F' => ghostty_key::Key::F,
        'g' | 'G' => ghostty_key::Key::G,
        'h' | 'H' => ghostty_key::Key::H,
        'i' | 'I' => ghostty_key::Key::I,
        'j' | 'J' => ghostty_key::Key::J,
        'k' | 'K' => ghostty_key::Key::K,
        'l' | 'L' => ghostty_key::Key::L,
        'm' | 'M' => ghostty_key::Key::M,
        'n' | 'N' => ghostty_key::Key::N,
        'o' | 'O' => ghostty_key::Key::O,
        'p' | 'P' => ghostty_key::Key::P,
        'q' | 'Q' => ghostty_key::Key::Q,
        'r' | 'R' => ghostty_key::Key::R,
        's' | 'S' => ghostty_key::Key::S,
        't' | 'T' => ghostty_key::Key::T,
        'u' | 'U' => ghostty_key::Key::U,
        'v' | 'V' => ghostty_key::Key::V,
        'w' | 'W' => ghostty_key::Key::W,
        'x' | 'X' => ghostty_key::Key::X,
        'y' | 'Y' => ghostty_key::Key::Y,
        'z' | 'Z' => ghostty_key::Key::Z,
        '-' => ghostty_key::Key::Minus,
        '.' => ghostty_key::Key::Period,
        '\'' => ghostty_key::Key::Quote,
        ';' => ghostty_key::Key::Semicolon,
        '/' => ghostty_key::Key::Slash,
        ' ' => ghostty_key::Key::Space,
        _ => ghostty_key::Key::Unidentified,
    }
}

fn function_key(n: u8) -> Option<ghostty_key::Key> {
    match n {
        1 => Some(ghostty_key::Key::F1),
        2 => Some(ghostty_key::Key::F2),
        3 => Some(ghostty_key::Key::F3),
        4 => Some(ghostty_key::Key::F4),
        5 => Some(ghostty_key::Key::F5),
        6 => Some(ghostty_key::Key::F6),
        7 => Some(ghostty_key::Key::F7),
        8 => Some(ghostty_key::Key::F8),
        9 => Some(ghostty_key::Key::F9),
        10 => Some(ghostty_key::Key::F10),
        11 => Some(ghostty_key::Key::F11),
        12 => Some(ghostty_key::Key::F12),
        13 => Some(ghostty_key::Key::F13),
        14 => Some(ghostty_key::Key::F14),
        15 => Some(ghostty_key::Key::F15),
        16 => Some(ghostty_key::Key::F16),
        17 => Some(ghostty_key::Key::F17),
        18 => Some(ghostty_key::Key::F18),
        19 => Some(ghostty_key::Key::F19),
        20 => Some(ghostty_key::Key::F20),
        21 => Some(ghostty_key::Key::F21),
        22 => Some(ghostty_key::Key::F22),
        23 => Some(ghostty_key::Key::F23),
        24 => Some(ghostty_key::Key::F24),
        25 => Some(ghostty_key::Key::F25),
        _ => None,
    }
}

fn media_key(media: MediaKeyCode) -> Option<ghostty_key::Key> {
    match media {
        MediaKeyCode::Play | MediaKeyCode::Pause | MediaKeyCode::PlayPause => {
            Some(ghostty_key::Key::MediaPlayPause)
        }
        MediaKeyCode::Stop => Some(ghostty_key::Key::MediaStop),
        MediaKeyCode::TrackNext => Some(ghostty_key::Key::MediaTrackNext),
        MediaKeyCode::TrackPrevious => Some(ghostty_key::Key::MediaTrackPrevious),
        _ => None,
    }
}

fn modifier_key(modifier: ModifierKeyCode) -> Option<ghostty_key::Key> {
    match modifier {
        ModifierKeyCode::LeftShift => Some(ghostty_key::Key::ShiftLeft),
        ModifierKeyCode::RightShift => Some(ghostty_key::Key::ShiftRight),
        ModifierKeyCode::LeftControl => Some(ghostty_key::Key::ControlLeft),
        ModifierKeyCode::RightControl => Some(ghostty_key::Key::ControlRight),
        ModifierKeyCode::LeftAlt => Some(ghostty_key::Key::AltLeft),
        ModifierKeyCode::RightAlt => Some(ghostty_key::Key::AltRight),
        ModifierKeyCode::LeftSuper | ModifierKeyCode::LeftMeta => Some(ghostty_key::Key::MetaLeft),
        ModifierKeyCode::RightSuper | ModifierKeyCode::RightMeta => {
            Some(ghostty_key::Key::MetaRight)
        }
        _ => None,
    }
}

fn encode_mouse_event(
    encoder: &mut ghostty_mouse::Encoder<'_>,
    vt: &Terminal<'_, '_>,
    mouse: MouseEvent,
    area: Rect,
) -> Result<Option<Vec<u8>>> {
    let Some(mut event) = crossterm_mouse_to_ghostty(mouse, area)? else {
        return Ok(None);
    };
    event.set_mods(crossterm_key_mods(mouse.modifiers));
    let mut encoded = Vec::with_capacity(32);
    encoder
        .set_options_from_terminal(vt)
        .set_size(mouse_encoder_size(area))
        .set_any_button_pressed(matches!(
            mouse.kind,
            MouseEventKind::Drag(_) | MouseEventKind::Down(_)
        ))
        .set_track_last_cell(true)
        .encode_to_vec(&event, &mut encoded)?;
    Ok((!encoded.is_empty()).then_some(encoded))
}

fn crossterm_mouse_to_ghostty(
    mouse: MouseEvent,
    area: Rect,
) -> Result<Option<ghostty_mouse::Event<'static>>> {
    if mouse.column < area.left()
        || mouse.column >= area.right()
        || mouse.row < area.top()
        || mouse.row >= area.bottom()
    {
        return Ok(None);
    }

    let (action, button) = match mouse.kind {
        MouseEventKind::Down(button) => (ghostty_mouse::Action::Press, Some(mouse_button(button))),
        MouseEventKind::Up(button) => (ghostty_mouse::Action::Release, Some(mouse_button(button))),
        MouseEventKind::Drag(button) => (ghostty_mouse::Action::Motion, Some(mouse_button(button))),
        MouseEventKind::Moved => (ghostty_mouse::Action::Motion, None),
        MouseEventKind::ScrollUp => (
            ghostty_mouse::Action::Press,
            Some(ghostty_mouse::Button::Four),
        ),
        MouseEventKind::ScrollDown => (
            ghostty_mouse::Action::Press,
            Some(ghostty_mouse::Button::Five),
        ),
        MouseEventKind::ScrollLeft => (
            ghostty_mouse::Action::Press,
            Some(ghostty_mouse::Button::Six),
        ),
        MouseEventKind::ScrollRight => (
            ghostty_mouse::Action::Press,
            Some(ghostty_mouse::Button::Seven),
        ),
    };

    let mut event = ghostty_mouse::Event::new()?;
    event
        .set_action(action)
        .set_button(button)
        .set_position(ghostty_mouse::Position {
            x: (mouse.column - area.x) as f32,
            y: (mouse.row - area.y) as f32,
        });
    Ok(Some(event))
}

fn mouse_button(button: MouseButton) -> ghostty_mouse::Button {
    match button {
        MouseButton::Left => ghostty_mouse::Button::Left,
        MouseButton::Right => ghostty_mouse::Button::Right,
        MouseButton::Middle => ghostty_mouse::Button::Middle,
    }
}

fn mouse_encoder_size(area: Rect) -> ghostty_mouse::EncoderSize {
    ghostty_mouse::EncoderSize {
        screen_width: area.width as u32,
        screen_height: area.height as u32,
        cell_width: 1,
        cell_height: 1,
        padding_top: 0,
        padding_bottom: 0,
        padding_right: 0,
        padding_left: 0,
    }
}

fn encode_focus_event(event: ghostty_focus::Event) -> Result<Vec<u8>> {
    let mut buf = [0_u8; 8];
    let len = event.encode(&mut buf)?;
    Ok(buf[..len].to_vec())
}

#[derive(Default)]
struct ShellInputTracker {
    line: String,
}

impl ShellInputTracker {
    fn handle_key(&mut self, key: KeyEvent, status: &StatusBar) -> bool {
        if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
            return false;
        }

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.line.clear();
                false
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.line.clear();
                false
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.line.push(c);
                false
            }
            KeyCode::Backspace => {
                self.line.pop();
                false
            }
            KeyCode::Enter => {
                let command = std::mem::take(&mut self.line);
                let current = status.cwd.borrow().clone();
                if let Some(cwd) = resolve_cd_command(&command, &current) {
                    *status.cwd.borrow_mut() = cwd;
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

fn resolve_cd_command(command: &str, current: &str) -> Option<String> {
    let words = split_shell_words(command)?;
    if words.first()? != "cd" || words.len() > 2 {
        return None;
    }
    let target = words.get(1).map(String::as_str).unwrap_or("~");
    if target == "-" {
        return None;
    }
    let path = expand_cd_target(target, current)?;
    path.is_dir()
        .then(|| normalize_path(&path).display().to_string())
}

fn expand_cd_target(target: &str, current: &str) -> Option<PathBuf> {
    if target == "~" {
        return env::var_os("HOME").map(PathBuf::from);
    }
    if let Some(rest) = target.strip_prefix("~/") {
        return env::var_os("HOME").map(|home| PathBuf::from(home).join(rest));
    }
    let path = PathBuf::from(target);
    if path.is_absolute() {
        Some(path)
    } else {
        Some(Path::new(current).join(path))
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn split_shell_words(input: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars();
    let mut quote = None;
    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (None, '\\') => current.push(chars.next()?),
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (None, '\'' | '"') => quote = Some(ch),
            (None, ch) if ch.is_whitespace() => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if quote.is_some() {
        return None;
    }
    if !current.is_empty() {
        words.push(current);
    }
    Some(words)
}

struct PtySession {
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Rc<RefCell<Box<dyn Write + Send>>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    output_rx: mpsc::Receiver<Vec<u8>>,
    _shell_integration: ShellIntegration,
}

impl PtySession {
    fn spawn(command: &[String], cols: u16, rows: u16) -> Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let program = command
            .first()
            .ok_or_else(|| anyhow!("missing command to spawn"))?;
        let shell_integration = ShellIntegration::new(command)?;
        let mut builder = CommandBuilder::new(shell_integration.program().unwrap_or(program));
        for arg in shell_integration.args() {
            builder.arg(arg);
        }
        for arg in command.iter().skip(1) {
            builder.arg(arg);
        }
        if let Ok(cwd) = env::current_dir() {
            builder.cwd(&cwd);
            builder.env("PWD", cwd.as_os_str());
        }
        builder.env(
            "TERM",
            env::var("INSIGNIA_CHILD_TERM")
                .or_else(|_| env::var("TERM"))
                .unwrap_or_else(|_| "xterm-ghostty".to_string()),
        );
        builder.env("INSIGNIA", "1");
        for (key, value) in shell_integration.envs() {
            builder.env(key, value);
        }

        let child = pair
            .slave
            .spawn_command(builder)
            .with_context(|| format!("failed to spawn {:?}", command))?;
        drop(pair.slave);

        let writer = Rc::new(RefCell::new(pair.master.take_writer()?));
        let reader = pair.master.try_clone_reader()?;
        let (output_tx, output_rx) = mpsc::channel();

        thread::Builder::new()
            .name("insignia-pty-reader".to_string())
            .spawn(move || read_pty(reader, output_tx))
            .context("failed to spawn PTY reader")?;

        Ok(Self {
            master: pair.master,
            writer,
            child,
            output_rx,
            _shell_integration: shell_integration,
        })
    }

    fn write_all(&self, bytes: &[u8]) -> Result<()> {
        let mut writer = self.writer.borrow_mut();
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    fn child_is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(Some(_)) => false,
            Ok(None) => true,
            Err(_) => false,
        }
    }
}

#[derive(Default)]
struct ShellIntegration {
    program: Option<String>,
    envs: Vec<(String, String)>,
    args: Vec<String>,
    cleanup: Vec<PathBuf>,
}

impl ShellIntegration {
    fn new(command: &[String]) -> Result<Self> {
        if env::var_os("INSIGNIA_DISABLE_SHELL_INTEGRATION").is_some() || command.len() != 1 {
            return Ok(Self::default());
        }

        let Some(program) = command.first() else {
            return Ok(Self::default());
        };
        let Some(shell) = Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
        else {
            return Ok(Self::default());
        };

        match shell {
            "sh" | "dash" | "ksh" => Self::for_posix_shell(program),
            "bash" => Self::for_bash(),
            "zsh" => Self::for_zsh(program),
            _ => Ok(Self::default()),
        }
    }

    fn for_posix_shell(program: &str) -> Result<Self> {
        let script = write_temp_file("sh", shell_integration_script(false))?;
        let wrapper = write_temp_file(
            "sh-wrapper",
            format!(
                "ENV={}\nexport ENV\nexec {} -i\n",
                shell_quote(&script.display().to_string()),
                shell_quote(program)
            ),
        )?;
        Ok(Self {
            program: Some("/bin/sh".to_string()),
            envs: Vec::new(),
            args: vec![wrapper.display().to_string()],
            cleanup: vec![script, wrapper],
        })
    }

    fn for_bash() -> Result<Self> {
        let script = write_temp_file("bash", shell_integration_script(true))?;
        let wrapper = write_temp_file(
            "bash-wrapper",
            format!(
                "exec bash --rcfile {} -i\n",
                shell_quote(&script.display().to_string())
            ),
        )?;
        Ok(Self {
            program: Some("/bin/sh".to_string()),
            envs: Vec::new(),
            args: vec![wrapper.display().to_string()],
            cleanup: vec![script, wrapper],
        })
    }

    fn for_zsh(program: &str) -> Result<Self> {
        let dir = temp_path("zsh");
        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        let script = dir.join(".zshrc");
        fs::write(&script, shell_integration_script(true))
            .with_context(|| format!("write {}", script.display()))?;
        let wrapper = write_temp_file(
            "zsh-wrapper",
            format!(
                "ZDOTDIR={}\nexport ZDOTDIR\nexec {} -i\n",
                shell_quote(&dir.display().to_string()),
                shell_quote(program)
            ),
        )?;
        Ok(Self {
            program: Some("/bin/sh".to_string()),
            envs: Vec::new(),
            args: vec![wrapper.display().to_string()],
            cleanup: vec![dir, wrapper],
        })
    }

    fn program(&self) -> Option<&str> {
        self.program.as_deref()
    }

    fn envs(&self) -> &[(String, String)] {
        &self.envs
    }

    fn args(&self) -> &[String] {
        &self.args
    }
}

impl Drop for ShellIntegration {
    fn drop(&mut self) {
        for path in &self.cleanup {
            if path.is_dir() {
                let _ = fs::remove_dir_all(path);
            } else {
                let _ = fs::remove_file(path);
            }
        }
    }
}

fn write_temp_file(kind: &str, contents: String) -> Result<PathBuf> {
    let path = temp_path(kind);
    fs::write(&path, contents).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

fn shell_quote(input: &str) -> String {
    format!("'{}'", input.replace('\'', "'\\''"))
}

fn temp_path(kind: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    env::temp_dir().join(format!("insignia-{kind}-{}-{nanos}", process::id()))
}

fn shell_integration_script(source_user_rc: bool) -> String {
    let mut script = String::new();
    if source_user_rc {
        script.push_str(
            r#"
if [ -n "${BASH_VERSION:-}" ] && [ -r "$HOME/.bashrc" ]; then
  . "$HOME/.bashrc"
elif [ -n "${ZSH_VERSION:-}" ] && [ -r "$HOME/.zshrc" ]; then
  . "$HOME/.zshrc"
fi
"#,
        );
    }
    script.push_str(
        r#"
__insignia_emit_pwd() {
  printf '\033]7;file://localhost%s\033\\' "$PWD"
}
cd() {
  command cd "$@" && __insignia_emit_pwd
}
__insignia_emit_pwd
"#,
    );
    script
}

impl Drop for PtySession {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn read_pty(mut reader: Box<dyn Read + Send>, output_tx: mpsc::Sender<Vec<u8>>) {
    let mut buf = [0_u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if output_tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        }
    }
}

struct GhosttyRenderer;

impl GhosttyRenderer {
    fn render(
        &mut self,
        vt: &mut Terminal<'static, 'static>,
        render_state: &mut RenderState<'static>,
        area: Rect,
        buf: &mut Buffer,
        cursor_text: RgbColor,
    ) {
        let Ok(snapshot) = render_state.update(vt) else {
            return;
        };
        let Ok(colors) = snapshot.colors() else {
            return;
        };
        let default_fg = rgb(colors.foreground);
        let cursor = render_cursor_from_snapshot(&snapshot, colors.cursor, cursor_text);
        let Ok(mut row_iter) = RowIterator::new() else {
            return;
        };
        let Ok(mut cell_iter) = CellIterator::new() else {
            return;
        };
        let Ok(mut rows) = row_iter.update(&snapshot) else {
            return;
        };

        let mut y = 0;
        while let Some(row) = rows.next() {
            if y >= area.height {
                break;
            }

            let Ok(mut cells) = cell_iter.update(row) else {
                y += 1;
                continue;
            };
            let mut x = 0;
            while let Some(cell) = cells.next() {
                if x >= area.width {
                    break;
                }

                let symbol = cell_symbol(&cell);
                let fg = cell
                    .fg_color()
                    .ok()
                    .flatten()
                    .map(rgb)
                    .unwrap_or(default_fg);
                let bg = cell.bg_color().ok().flatten().map(rgb);
                let cell = &mut buf[(area.x + x, area.y + y)];
                cell.set_symbol(&symbol);
                cell.set_style(terminal_cell_style(fg, bg));
                x += 1;
            }
            y += 1;
        }

        if let Some(cursor) = cursor {
            render_cursor(cursor, area, buf);
        }
    }
}

#[derive(Clone, Copy)]
struct RenderCursor {
    pos: CellPos,
    color: RgbColor,
    text: RgbColor,
    style: CursorVisualStyle,
}

fn render_cursor_from_snapshot(
    snapshot: &libghostty_vt::render::Snapshot<'_, '_>,
    color: Option<RgbColor>,
    text: RgbColor,
) -> Option<RenderCursor> {
    if !snapshot.cursor_visible().ok()? {
        return None;
    }
    let viewport = snapshot.cursor_viewport().ok()??;
    let style = snapshot.cursor_visual_style().ok()?;
    let color = snapshot.cursor_color().ok().flatten().or(color)?;
    Some(RenderCursor {
        pos: CellPos {
            col: viewport.x,
            row: viewport.y,
        },
        color,
        text,
        style,
    })
}

fn render_cursor(cursor: RenderCursor, area: Rect, buf: &mut Buffer) {
    let Some(pos) = constrain_cell(cursor.pos, area) else {
        return;
    };
    let cell = &mut buf[(area.x + pos.col, area.y + pos.row)];
    match cursor.style {
        CursorVisualStyle::Underline => {
            cell.set_fg(rgb(cursor.color));
            cell.set_style(cell.style().add_modifier(Modifier::UNDERLINED));
        }
        CursorVisualStyle::Bar => {
            cell.set_symbol("|");
            cell.set_fg(rgb(cursor.color));
        }
        CursorVisualStyle::Block | CursorVisualStyle::BlockHollow => {
            cell.set_fg(rgb(cursor.text));
            cell.set_bg(rgb(cursor.color));
        }
        _ => {
            cell.set_fg(rgb(cursor.text));
            cell.set_bg(rgb(cursor.color));
        }
    }
}

fn terminal_cell_style(fg: Color, bg: Option<Color>) -> Style {
    let style = Style::default().fg(fg);
    if let Some(bg) = bg {
        style.bg(bg)
    } else {
        style.bg(Color::Reset)
    }
}

fn cell_symbol(cell: &libghostty_vt::render::CellIteration<'_, '_>) -> String {
    match cell.graphemes_len() {
        Ok(0) | Err(_) => " ".to_string(),
        Ok(_) => {
            let mut symbol = String::new();
            if cell.graphemes_utf8(&mut symbol).is_ok() && !symbol.is_empty() {
                symbol
            } else {
                " ".to_string()
            }
        }
    }
}

fn rgb(color: RgbColor) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellPos {
    col: u16,
    row: u16,
}

#[derive(Debug, Clone, Copy)]
struct InteractionStyle {
    foreground: RgbColor,
    background: RgbColor,
    hover_background: RgbColor,
}

impl Default for InteractionStyle {
    fn default() -> Self {
        let theme = Theme::default();
        theme.interaction_style()
    }
}

#[derive(Debug, Clone)]
struct InteractionOverlay {
    anchor: Option<CellPos>,
    focus: Option<CellPos>,
    hover: Option<CellPos>,
    selecting: bool,
    style: InteractionStyle,
}

impl Default for InteractionOverlay {
    fn default() -> Self {
        Self {
            anchor: None,
            focus: None,
            hover: None,
            selecting: false,
            style: InteractionStyle::default(),
        }
    }
}

impl InteractionOverlay {
    fn set_theme(&mut self, style: InteractionStyle) {
        self.style = style;
    }

    fn clear(&mut self) -> bool {
        let had_state = self.anchor.is_some() || self.focus.is_some() || self.hover.is_some();
        self.anchor = None;
        self.focus = None;
        self.hover = None;
        self.selecting = false;
        had_state
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, area: Rect) -> bool {
        let pos = mouse_to_cell(mouse, area);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(pos) = pos else {
                    return self.clear();
                };
                self.anchor = Some(pos);
                self.focus = Some(pos);
                self.hover = None;
                self.selecting = true;
                true
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some(pos) = pos else {
                    return false;
                };
                if self.focus != Some(pos) || !self.selecting {
                    self.focus = Some(pos);
                    self.selecting = true;
                    return true;
                }
                false
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(pos) = pos {
                    self.focus = Some(pos);
                }
                if self.selecting {
                    self.selecting = false;
                    return true;
                }
                false
            }
            MouseEventKind::Moved => {
                if self.selecting || self.selected_range().is_some() {
                    return false;
                }
                if self.hover != pos {
                    self.hover = pos;
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    fn constrain_to(&mut self, area: Rect) {
        self.anchor = self.anchor.and_then(|pos| constrain_cell(pos, area));
        self.focus = self.focus.and_then(|pos| constrain_cell(pos, area));
        self.hover = self.hover.and_then(|pos| constrain_cell(pos, area));
        if self.anchor.is_none() || self.focus.is_none() {
            self.anchor = None;
            self.focus = None;
            self.selecting = false;
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if let Some(range) = self.selected_range() {
            for_each_cell_in_range(range, area, |pos| {
                let cell = &mut buf[(area.x + pos.col, area.y + pos.row)];
                cell.set_fg(rgb(self.style.foreground));
                cell.set_bg(rgb(self.style.background));
            });
        }

        if !self.selecting
            && let Some(hover) = self.hover
            && constrain_cell(hover, area).is_some()
        {
            let cell = &mut buf[(area.x + hover.col, area.y + hover.row)];
            if !self
                .selected_range()
                .is_some_and(|range| range.contains(hover))
            {
                cell.set_bg(rgb(self.style.hover_background));
            }
        }
    }

    fn selected_range(&self) -> Option<CellRange> {
        let anchor = self.anchor?;
        let focus = self.focus?;
        Some(CellRange::new(anchor, focus))
    }
}

fn for_each_cell_in_range(range: CellRange, area: Rect, mut visit: impl FnMut(CellPos)) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let start_row = range.start.row.min(area.height - 1);
    let end_row = range.end.row.min(area.height - 1);
    for row in start_row..=end_row {
        let start_col = if row == range.start.row {
            range.start.col
        } else {
            0
        };
        let end_col = if row == range.end.row {
            range.end.col
        } else {
            area.width - 1
        };
        for col in start_col.min(area.width - 1)..=end_col.min(area.width - 1) {
            visit(CellPos { col, row });
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellRange {
    start: CellPos,
    end: CellPos,
}

impl CellRange {
    fn new(a: CellPos, b: CellPos) -> Self {
        if cell_index(a) <= cell_index(b) {
            Self { start: a, end: b }
        } else {
            Self { start: b, end: a }
        }
    }

    fn contains(self, pos: CellPos) -> bool {
        let pos = cell_index(pos);
        cell_index(self.start) <= pos && pos <= cell_index(self.end)
    }
}

fn cell_index(pos: CellPos) -> u32 {
    ((pos.row as u32) << 16) | pos.col as u32
}

fn mouse_to_cell(mouse: MouseEvent, area: Rect) -> Option<CellPos> {
    if mouse.column < area.left()
        || mouse.column >= area.right()
        || mouse.row < area.top()
        || mouse.row >= area.bottom()
    {
        return None;
    }

    Some(CellPos {
        col: mouse.column - area.x,
        row: mouse.row - area.y,
    })
}

fn constrain_cell(pos: CellPos, area: Rect) -> Option<CellPos> {
    if pos.col < area.width && pos.row < area.height {
        Some(pos)
    } else {
        None
    }
}

#[derive(Clone, Debug)]
struct Theme {
    foreground: RgbColor,
    background: RgbColor,
    cursor_color: RgbColor,
    cursor_text: RgbColor,
    selection_foreground: RgbColor,
    selection_background: RgbColor,
    palette: [RgbColor; 256],
    background_image: Option<String>,
    background_image_opacity: Option<f32>,
    theme_setting: Option<String>,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            foreground: RgbColor {
                r: 0xdd,
                g: 0xdd,
                b: 0xdd,
            },
            background: RgbColor {
                r: 0x10,
                g: 0x10,
                b: 0x10,
            },
            cursor_color: RgbColor {
                r: 0xdd,
                g: 0xdd,
                b: 0xdd,
            },
            cursor_text: RgbColor {
                r: 0x10,
                g: 0x10,
                b: 0x10,
            },
            selection_foreground: RgbColor {
                r: 0xdd,
                g: 0xdd,
                b: 0xdd,
            },
            selection_background: RgbColor {
                r: 0x44,
                g: 0x44,
                b: 0x44,
            },
            palette: default_palette(),
            background_image: None,
            background_image_opacity: None,
            theme_setting: None,
        }
    }
}

impl Theme {
    fn query() -> Result<Self> {
        if env::var_os("INSIGNIA_DISABLE_THEME_QUERY").is_some() {
            info!("theme query disabled by INSIGNIA_DISABLE_THEME_QUERY");
            return Ok(Self::default());
        }

        info!(
            path = ?env::var_os("PATH"),
            home = ?env::var_os("HOME"),
            xdg_config_home = ?env::var_os("XDG_CONFIG_HOME"),
            "theme query started"
        );

        let mut theme = match Self::from_ghostty_config() {
            Ok(theme) => {
                info!(
                    theme_setting = ?theme.theme_setting,
                    foreground = %format_rgb(theme.foreground),
                    background = %format_rgb(theme.background),
                    selection_foreground = %format_rgb(theme.selection_foreground),
                    selection_background = %format_rgb(theme.selection_background),
                    "loaded theme from ghostty +show-config"
                );
                theme
            }
            Err(error) => {
                warn!(%error, "ghostty +show-config theme load failed; using defaults");
                Self::default()
            }
        };

        if env::var_os("INSIGNIA_DISABLE_OSC_THEME_QUERY").is_some() {
            info!("OSC theme query disabled by INSIGNIA_DISABLE_OSC_THEME_QUERY");
        } else {
            match OpenOptions::new().read(true).write(true).open("/dev/tty") {
                Ok(mut tty) => match send_color_queries(&mut tty) {
                    Ok(()) => {
                        let started = Instant::now();
                        match read_color_responses(&mut tty, &mut theme, COLOR_QUERY_TIMEOUT) {
                            Ok(parsed) => {
                                info!(
                                    parsed,
                                    elapsed_ms = started.elapsed().as_millis(),
                                    foreground = %format_rgb(theme.foreground),
                                    background = %format_rgb(theme.background),
                                    cursor = %format_rgb(theme.cursor_color),
                                    "OSC theme query completed"
                                );
                            }
                            Err(error) => {
                                warn!(%error, "OSC theme response read failed");
                            }
                        }
                    }
                    Err(error) => {
                        warn!(%error, "failed to send OSC theme queries");
                    }
                },
                Err(error) => {
                    warn!(%error, "failed to open /dev/tty for OSC theme query");
                }
            }
        }

        let (appearance, appearance_source) = if let Some(appearance) = Appearance::from_env() {
            (appearance, "INSIGNIA_GHOSTTY_APPEARANCE")
        } else {
            match Appearance::from_macos() {
                Ok(Some(appearance)) => (appearance, "macOS AppleInterfaceStyle"),
                Ok(None) => (
                    Appearance::from_background(theme.background),
                    "theme background luminance",
                ),
                Err(error) => {
                    warn!(%error, "failed to read macOS appearance");
                    (
                        Appearance::from_background(theme.background),
                        "theme background luminance",
                    )
                }
            }
        };
        info!(?appearance, appearance_source, "resolved theme appearance");
        match apply_theme_file_for_appearance(&mut theme, appearance) {
            Ok(Some(path)) => {
                info!(
                    path = %path.display(),
                    foreground = %format_rgb(theme.foreground),
                    background = %format_rgb(theme.background),
                    selection_foreground = %format_rgb(theme.selection_foreground),
                    selection_background = %format_rgb(theme.selection_background),
                    "applied ghostty theme file"
                );
            }
            Ok(None) => {
                info!("no ghostty theme file applied");
            }
            Err(error) => {
                warn!(%error, "failed to apply ghostty theme file");
            }
        }
        Ok(theme)
    }

    fn from_ghostty_config() -> Result<Self> {
        let mut command = Command::new("ghostty");
        command
            .arg("+show-config")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let started = Instant::now();
        let output = command_output_timeout(command, GHOSTTY_CONFIG_TIMEOUT)
            .context("run ghostty +show-config")?;
        debug!(
            status = ?output.status.code(),
            elapsed_ms = started.elapsed().as_millis(),
            stderr = %String::from_utf8_lossy(&output.stderr),
            "ghostty +show-config exited"
        );
        if !output.status.success() {
            return Err(anyhow!("ghostty +show-config failed: {}", output.status));
        }

        let config = String::from_utf8(output.stdout).context("parse ghostty config as utf-8")?;
        Ok(parse_ghostty_config(&config))
    }

    fn apply_to_vt(&self, vt: &mut Terminal<'_, '_>) -> Result<()> {
        vt.set_default_fg_color(Some(self.foreground))?
            .set_default_bg_color(Some(self.background))?
            .set_default_cursor_color(Some(self.cursor_color))?
            .set_default_color_palette(Some(self.palette))?;
        Ok(())
    }

    fn status_style(&self) -> StatusStyle {
        StatusStyle {
            foreground: self.selection_foreground,
            background: self.selection_background,
        }
    }

    fn interaction_style(&self) -> InteractionStyle {
        InteractionStyle {
            foreground: self.selection_foreground,
            background: self.selection_background,
            hover_background: blend(self.background, self.selection_background, 0.35),
        }
    }
}

fn command_output_timeout(mut command: Command, timeout: Duration) -> Result<Output> {
    let mut child = command.spawn()?;
    let deadline = Instant::now() + timeout;

    loop {
        if child.try_wait()?.is_some() {
            return Ok(child.wait_with_output()?);
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow!("command timed out after {}ms", timeout.as_millis()));
        }

        thread::sleep(Duration::from_millis(5));
    }
}

fn send_color_queries(tty: &mut impl Write) -> io::Result<()> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"\x1b]10;?\x1b\\");
    buf.extend_from_slice(b"\x1b]11;?\x1b\\");
    buf.extend_from_slice(b"\x1b]12;?\x1b\\");
    for i in 0..ANSI_PALETTE_QUERY_COUNT {
        write!(buf, "\x1b]4;{i};?\x1b\\")?;
    }
    tty.write_all(&buf)?;
    tty.flush()
}

fn parse_ghostty_config(config: &str) -> Theme {
    let mut theme = Theme::default();
    apply_ghostty_config(config, &mut theme);
    theme
}

fn apply_ghostty_config(config: &str, theme: &mut Theme) {
    for line in config.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();

        match key {
            "theme" => theme.theme_setting = Some(value.to_string()),
            "background" => update_color(value, &mut theme.background),
            "foreground" => update_color(value, &mut theme.foreground),
            "cursor-color" => update_color(value, &mut theme.cursor_color),
            "cursor-text" => update_color(value, &mut theme.cursor_text),
            "selection-foreground" => update_color(value, &mut theme.selection_foreground),
            "selection-background" => update_color(value, &mut theme.selection_background),
            "background-image" => theme.background_image = Some(value.to_string()),
            "background-image-opacity" => {
                theme.background_image_opacity = value.parse::<f32>().ok();
            }
            "palette" => {
                let Some((index, color)) = value.split_once('=') else {
                    continue;
                };
                let Ok(index) = index.trim().parse::<usize>() else {
                    continue;
                };
                if index < theme.palette.len()
                    && let Some(color) = parse_hex_rgb(color.trim())
                {
                    theme.palette[index] = color;
                }
            }
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Appearance {
    Light,
    Dark,
}

impl Appearance {
    fn from_env() -> Option<Self> {
        match env::var("INSIGNIA_GHOSTTY_APPEARANCE").ok()?.as_str() {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    fn from_macos() -> Result<Option<Self>> {
        let mut command = Command::new("defaults");
        command
            .args(["read", "-g", "AppleInterfaceStyle"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let started = Instant::now();
        let output = command_output_timeout(command, MACOS_APPEARANCE_TIMEOUT)
            .context("run defaults read -g AppleInterfaceStyle")?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        debug!(
            status = ?output.status.code(),
            elapsed_ms = started.elapsed().as_millis(),
            stdout = %stdout.trim(),
            stderr = %stderr.trim(),
            "defaults AppleInterfaceStyle exited"
        );

        if output.status.success() {
            if stdout.trim().eq_ignore_ascii_case("dark") {
                Ok(Some(Self::Dark))
            } else {
                Ok(Some(Self::Light))
            }
        } else if stderr.contains("does not exist") {
            Ok(Some(Self::Light))
        } else {
            Ok(None)
        }
    }

    fn from_background(color: RgbColor) -> Self {
        if color_luminance(color) < 0.5 {
            Self::Dark
        } else {
            Self::Light
        }
    }
}

fn color_luminance(color: RgbColor) -> f32 {
    ((0.2126 * color.r as f32) + (0.7152 * color.g as f32) + (0.0722 * color.b as f32)) / 255.0
}

fn format_rgb(color: RgbColor) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
}

fn apply_theme_file_for_appearance(
    theme: &mut Theme,
    appearance: Appearance,
) -> Result<Option<PathBuf>> {
    apply_theme_file_for_appearance_from_dirs(theme, appearance, &ghostty_theme_dirs())
}

fn apply_theme_file_for_appearance_from_dirs(
    theme: &mut Theme,
    appearance: Appearance,
    dirs: &[PathBuf],
) -> Result<Option<PathBuf>> {
    let Some(setting) = theme.theme_setting.clone() else {
        return Ok(None);
    };
    let Some(name) = theme_name_for_appearance(&setting, appearance) else {
        return Ok(None);
    };

    for dir in dirs {
        for path in ghostty_theme_candidates(dir, name) {
            debug!(path = %path.display(), "checking ghostty theme candidate");
            if path.is_file() {
                let config = fs::read_to_string(&path)
                    .with_context(|| format!("read {}", path.display()))?;
                apply_ghostty_config(&config, theme);
                return Ok(Some(path));
            }
        }
    }

    Ok(None)
}

fn ghostty_theme_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".config/ghostty/themes"));
    }
    if let Some(config_home) = env::var_os("XDG_CONFIG_HOME") {
        dirs.push(PathBuf::from(config_home).join("ghostty/themes"));
    }
    dirs
}

fn ghostty_theme_candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    vec![dir.join(name), dir.join(format!("{name}.conf"))]
}

fn theme_name_for_appearance(setting: &str, appearance: Appearance) -> Option<&str> {
    let mut fallback = None;
    for part in setting
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let Some((prefix, name)) = part.split_once(':') else {
            fallback = Some(part);
            continue;
        };

        let prefix = prefix.trim();
        let name = name.trim();
        if (appearance == Appearance::Light && prefix == "light")
            || (appearance == Appearance::Dark && prefix == "dark")
        {
            return Some(name);
        }
    }
    fallback
}

fn update_color(value: &str, target: &mut RgbColor) {
    if let Some(color) = parse_hex_rgb(value) {
        *target = color;
    }
}

fn read_color_responses(
    tty: &mut (impl Read + AsRawFd),
    theme: &mut Theme,
    timeout: Duration,
) -> io::Result<usize> {
    let fd = tty.as_raw_fd();
    let original_flags = set_nonblocking(fd)?;
    let result = read_color_responses_nonblocking(tty, theme, timeout);
    let restore_result = restore_fd_flags(fd, original_flags);
    let parsed = result?;
    restore_result?;
    Ok(parsed)
}

fn read_color_responses_nonblocking(
    tty: &mut (impl Read + AsRawFd),
    theme: &mut Theme,
    timeout: Duration,
) -> io::Result<usize> {
    let deadline = Instant::now() + timeout;
    let mut buf = Vec::new();
    let mut tmp = [0_u8; 1024];
    let mut seen_response = false;
    let mut parsed = 0;

    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let wait = if seen_response {
            remaining.min(COLOR_QUERY_QUIET)
        } else {
            remaining
        };

        if !poll_read(tty.as_raw_fd(), wait)? {
            break;
        }

        loop {
            match tty.read(&mut tmp) {
                Ok(0) => return Ok(parsed),
                Ok(n) => {
                    seen_response = true;
                    buf.extend_from_slice(&tmp[..n]);
                    parsed += consume_osc_color_responses(&mut buf, theme);
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) => return Err(err),
            }
        }
    }

    Ok(parsed)
}

fn set_nonblocking(fd: i32) -> io::Result<i32> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(flags)
}

fn restore_fd_flags(fd: i32, flags: i32) -> io::Result<()> {
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags) } == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn poll_read(fd: i32, timeout: Duration) -> io::Result<bool> {
    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    match unsafe { libc::poll(&raw mut pfd, 1, ms) } {
        -1 => Err(io::Error::last_os_error()),
        0 => Ok(false),
        _ => Ok(true),
    }
}

fn consume_osc_color_responses(buf: &mut Vec<u8>, theme: &mut Theme) -> usize {
    let mut parsed = 0;
    loop {
        let Some(start) = buf.windows(2).position(|window| window == b"\x1b]") else {
            buf.clear();
            break parsed;
        };
        let content_start = start + 2;
        let terminator = buf[content_start..]
            .iter()
            .enumerate()
            .find_map(|(offset, byte)| {
                let index = content_start + offset;
                if *byte == 0x07 {
                    Some((index, 1))
                } else if *byte == 0x1b && buf.get(index + 1) == Some(&b'\\') {
                    Some((index, 2))
                } else {
                    None
                }
            });

        let Some((end, terminator_len)) = terminator else {
            buf.drain(..start);
            break parsed;
        };

        if let Ok(content) = std::str::from_utf8(&buf[content_start..end]) {
            if let Some(command) = apply_osc_color(content, theme) {
                parsed += 1;
                debug!(command, "applied OSC color response");
            }
        }
        buf.drain(..end + terminator_len);
    }
}

fn apply_osc_color(content: &str, theme: &mut Theme) -> Option<u16> {
    let (command, rest) = content.split_once(';')?;
    let command = command.parse::<u16>().ok()?;
    match command {
        10 => theme.foreground = parse_rgb(rest)?,
        11 => theme.background = parse_rgb(rest)?,
        12 => theme.cursor_color = parse_rgb(rest)?,
        4 => {
            let (index, color) = rest.split_once(';')?;
            let index = index.parse::<usize>().ok()?;
            if index < theme.palette.len() {
                theme.palette[index] = parse_rgb(color)?;
            }
        }
        _ => return None,
    }
    Some(command)
}

fn parse_hex_rgb(input: &str) -> Option<RgbColor> {
    let hex = input.trim().strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }

    Some(RgbColor {
        r: u8::from_str_radix(&hex[0..2], 16).ok()?,
        g: u8::from_str_radix(&hex[2..4], 16).ok()?,
        b: u8::from_str_radix(&hex[4..6], 16).ok()?,
    })
}

fn parse_rgb(input: &str) -> Option<RgbColor> {
    let channels = input.strip_prefix("rgb:")?;
    let mut parts = channels.splitn(3, '/');
    Some(RgbColor {
        r: parse_rgb_channel(parts.next()?)?,
        g: parse_rgb_channel(parts.next()?)?,
        b: parse_rgb_channel(parts.next()?)?,
    })
}

fn parse_rgb_channel(input: &str) -> Option<u8> {
    let trimmed = input.trim();
    let value = u32::from_str_radix(trimmed, 16).ok()?;
    Some(match trimmed.len() {
        1 => (value * 17) as u8,
        2 => value as u8,
        3 => (value >> 4) as u8,
        4 => (value >> 8) as u8,
        _ => return None,
    })
}

fn default_palette() -> [RgbColor; 256] {
    let mut palette = [RgbColor { r: 0, g: 0, b: 0 }; 256];
    let base = [
        (PaletteIndex::BLACK.0 as usize, (0x00, 0x00, 0x00)),
        (PaletteIndex::RED.0 as usize, (0xcd, 0x31, 0x31)),
        (PaletteIndex::GREEN.0 as usize, (0x0d, 0xbc, 0x79)),
        (PaletteIndex::YELLOW.0 as usize, (0xe5, 0xe5, 0x10)),
        (PaletteIndex::BLUE.0 as usize, (0x24, 0x71, 0xd1)),
        (PaletteIndex::MAGENTA.0 as usize, (0xbc, 0x3f, 0xbc)),
        (PaletteIndex::CYAN.0 as usize, (0x11, 0xa8, 0xcd)),
        (PaletteIndex::WHITE.0 as usize, (0xe5, 0xe5, 0xe5)),
        (PaletteIndex::BRIGHT_BLACK.0 as usize, (0x66, 0x66, 0x66)),
        (PaletteIndex::BRIGHT_RED.0 as usize, (0xf1, 0x4c, 0x4c)),
        (PaletteIndex::BRIGHT_GREEN.0 as usize, (0x23, 0xd1, 0x8b)),
        (PaletteIndex::BRIGHT_YELLOW.0 as usize, (0xf5, 0xf5, 0x43)),
        (PaletteIndex::BRIGHT_BLUE.0 as usize, (0x3b, 0x8e, 0xff)),
        (PaletteIndex::BRIGHT_MAGENTA.0 as usize, (0xd6, 0x70, 0xd6)),
        (PaletteIndex::BRIGHT_CYAN.0 as usize, (0x29, 0xb8, 0xdb)),
        (PaletteIndex::BRIGHT_WHITE.0 as usize, (0xff, 0xff, 0xff)),
    ];

    for (index, (r, g, b)) in base {
        palette[index] = RgbColor { r, g, b };
    }

    for value in 16..232 {
        let n = value - 16;
        let r = color_cube_channel(n / 36);
        let g = color_cube_channel((n / 6) % 6);
        let b = color_cube_channel(n % 6);
        palette[value] = RgbColor { r, g, b };
    }

    for value in 232..256 {
        let level = 8 + ((value - 232) * 10) as u8;
        palette[value] = RgbColor {
            r: level,
            g: level,
            b: level,
        };
    }

    palette
}

fn color_cube_channel(value: usize) -> u8 {
    if value == 0 {
        0
    } else {
        (55 + value * 40) as u8
    }
}

fn blend(a: RgbColor, b: RgbColor, b_weight: f32) -> RgbColor {
    let a_weight = 1.0 - b_weight;
    RgbColor {
        r: ((a.r as f32 * a_weight) + (b.r as f32 * b_weight)).round() as u8,
        g: ((a.g as f32 * a_weight) + (b.g as f32 * b_weight)).round() as u8,
        b: ((a.b as f32 * a_weight) + (b.b as f32 * b_weight)).round() as u8,
    }
}

#[derive(Clone, Copy)]
struct StatusStyle {
    foreground: RgbColor,
    background: RgbColor,
}

impl Default for StatusStyle {
    fn default() -> Self {
        Theme::default().status_style()
    }
}

struct StatusBar {
    label: String,
    cwd: Rc<RefCell<String>>,
    style: StatusStyle,
}

impl StatusBar {
    fn new() -> Result<Self> {
        let label = env::var("INSIGNIA_STATUS_LABEL").unwrap_or_else(|_| "insignia".to_string());
        let cwd = env::var("INSIGNIA_STATUS_CWD").unwrap_or_else(|_| {
            env::current_dir()
                .ok()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "?".to_string())
        });
        Ok(Self {
            label,
            cwd: Rc::new(RefCell::new(cwd)),
            style: StatusStyle::default(),
        })
    }

    fn set_theme(&mut self, style: StatusStyle) {
        self.style = status_style_from_env().unwrap_or(style);
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }

        let y = area.bottom() - 1;
        let style = Style::default()
            .fg(rgb(self.style.foreground))
            .bg(rgb(self.style.background))
            .add_modifier(Modifier::BOLD);
        for x in area.left()..area.right() {
            let cell = &mut buf[(x, y)];
            cell.set_symbol(" ");
            cell.set_style(style);
        }

        let time = env::var("INSIGNIA_TEST_TIME")
            .unwrap_or_else(|_| Local::now().format("%H:%M").to_string());
        let cwd = self.cwd.borrow();
        let text = format!(" {}  |  {}  |  {} ", self.label, cwd, time);
        write_clipped(buf, area.x, y, area.width, &text, style);
    }
}

fn status_style_from_env() -> Option<StatusStyle> {
    let foreground = env::var("INSIGNIA_STATUS_FOREGROUND")
        .ok()
        .and_then(|value| parse_hex_rgb(&value));
    let background = env::var("INSIGNIA_STATUS_BACKGROUND")
        .ok()
        .and_then(|value| parse_hex_rgb(&value));

    match (foreground, background) {
        (Some(foreground), Some(background)) => Some(StatusStyle {
            foreground,
            background,
        }),
        _ => None,
    }
}

fn display_pwd(pwd: &str) -> String {
    let Some(path) = pwd.strip_prefix("file://") else {
        return pwd.to_string();
    };
    let path = path.find('/').map(|index| &path[index..]).unwrap_or(path);
    percent_decode(path)
}

fn notify_outer_terminal_pwd(pwd: &str) {
    let Some(sequence) = outer_terminal_pwd_sequence(pwd) else {
        return;
    };

    match OpenOptions::new().write(true).open("/dev/tty") {
        Ok(mut tty) => {
            if let Err(error) = tty.write_all(sequence.as_bytes()).and_then(|_| tty.flush()) {
                warn!(%error, "failed to notify outer terminal of cwd");
            }
        }
        Err(error) => {
            warn!(%error, "failed to open /dev/tty for outer cwd notification");
        }
    }
}

fn outer_terminal_pwd_sequence(pwd: &str) -> Option<String> {
    if !pwd.starts_with("file://") || pwd.bytes().any(|byte| byte < 0x20 || byte == 0x7f) {
        return None;
    }

    Some(format!("\x1b]7;{pwd}\x1b\\"))
}

fn percent_decode(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Some(decoded) = decode_hex_byte(bytes[index + 1], bytes[index + 2])
        {
            output.push(decoded as char);
            index += 3;
        } else {
            output.push(bytes[index] as char);
            index += 1;
        }
    }
    output
}

fn decode_hex_byte(high: u8, low: u8) -> Option<u8> {
    Some((hex_value(high)? << 4) | hex_value(low)?)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn write_clipped(buf: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    for (offset, ch) in text.chars().take(width as usize).enumerate() {
        let cell = &mut buf[(x + offset as u16, y)];
        cell.set_char(ch);
        cell.set_style(style);
    }
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        TERMINAL_RESTORED.store(false, Ordering::SeqCst);
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = restore_terminal();
            previous(info);
        }));
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = restore_terminal();
    }
}

fn restore_terminal() -> Result<()> {
    if TERMINAL_RESTORED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    terminal::disable_raw_mode()?;
    execute!(
        io::stdout(),
        DisableMouseCapture,
        DisableFocusChange,
        DisableBracketedPaste,
        PopKeyboardEnhancementFlags,
        cursor::Show,
        SetCursorStyle::DefaultUserShape
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_two_digit_rgb_response() {
        assert_eq!(
            parse_rgb("rgb:AA/11/22"),
            Some(RgbColor {
                r: 0xaa,
                g: 0x11,
                b: 0x22
            })
        );
    }

    #[test]
    fn parses_four_digit_rgb_response() {
        assert_eq!(
            parse_rgb("rgb:FFFF/8080/0000"),
            Some(RgbColor {
                r: 0xff,
                g: 0x80,
                b: 0x00
            })
        );
    }

    #[test]
    fn consumes_osc_theme_responses() {
        let mut theme = Theme::default();
        let mut buf = b"\x1b]10;rgb:EEEE/EEEE/EEEE\x1b\\\x1b]11;rgb:1010/1010/1010\x1b\\\x1b]12;rgb:F5CC/3636/0000\x1b\\\x1b]4;1;rgb:AA/11/22\x07".to_vec();

        consume_osc_color_responses(&mut buf, &mut theme);

        assert_eq!(
            theme.foreground,
            RgbColor {
                r: 0xee,
                g: 0xee,
                b: 0xee
            }
        );
        assert_eq!(
            theme.background,
            RgbColor {
                r: 0x10,
                g: 0x10,
                b: 0x10
            }
        );
        assert_eq!(
            theme.palette[1],
            RgbColor {
                r: 0xaa,
                g: 0x11,
                b: 0x22
            }
        );
        assert_eq!(
            theme.cursor_color,
            RgbColor {
                r: 0xf5,
                g: 0x36,
                b: 0x00
            }
        );
        assert!(buf.is_empty());
    }

    #[test]
    fn startup_color_query_only_requests_ansi_palette() {
        let mut buf = Vec::new();
        send_color_queries(&mut buf).unwrap();
        let query = String::from_utf8(buf).unwrap();

        assert!(query.contains("\x1b]10;?\x1b\\"));
        assert!(query.contains("\x1b]11;?\x1b\\"));
        assert!(query.contains("\x1b]12;?\x1b\\"));
        assert!(query.contains("\x1b]4;0;?\x1b\\"));
        assert!(query.contains("\x1b]4;15;?\x1b\\"));
        assert!(!query.contains("\x1b]4;16;?\x1b\\"));
        assert!(!query.contains("\x1b]4;255;?\x1b\\"));
    }

    #[test]
    fn parses_ghostty_show_config_theme_values() {
        let theme = parse_ghostty_config(
            r#"
theme = light:Catppuccin Latte,dark:test
background = #131313
foreground = #ffffff
background-image = /Users/heyglassy/.config/ghostty/backgrounds/insignia.png
background-image-opacity = 0.08
selection-background = #1f0f02
selection-foreground = #f0d77d
cursor-color = #f5cc36
cursor-text = #505050
palette = 1=#a71f00
palette = 15=#f7f7f7
"#,
        );

        assert_eq!(
            theme.background,
            RgbColor {
                r: 0x13,
                g: 0x13,
                b: 0x13
            }
        );
        assert_eq!(
            theme.foreground,
            RgbColor {
                r: 0xff,
                g: 0xff,
                b: 0xff
            }
        );
        assert_eq!(
            theme.selection_background,
            RgbColor {
                r: 0x1f,
                g: 0x0f,
                b: 0x02
            }
        );
        assert_eq!(
            theme.selection_foreground,
            RgbColor {
                r: 0xf0,
                g: 0xd7,
                b: 0x7d
            }
        );
        assert_eq!(
            theme.cursor_color,
            RgbColor {
                r: 0xf5,
                g: 0xcc,
                b: 0x36
            }
        );
        assert_eq!(
            theme.cursor_text,
            RgbColor {
                r: 0x50,
                g: 0x50,
                b: 0x50
            }
        );
        assert_eq!(
            theme.palette[1],
            RgbColor {
                r: 0xa7,
                g: 0x1f,
                b: 0x00
            }
        );
        assert_eq!(
            theme.palette[15],
            RgbColor {
                r: 0xf7,
                g: 0xf7,
                b: 0xf7
            }
        );
        assert_eq!(
            theme.background_image.as_deref(),
            Some("/Users/heyglassy/.config/ghostty/backgrounds/insignia.png")
        );
        assert_eq!(theme.background_image_opacity, Some(0.08));
    }

    #[test]
    fn theme_name_for_appearance_uses_matching_pair_member() {
        assert_eq!(
            theme_name_for_appearance("light:Catppuccin Latte,dark:test", Appearance::Dark),
            Some("test")
        );
        assert_eq!(
            theme_name_for_appearance("dark:test,light:Catppuccin Latte", Appearance::Light),
            Some("Catppuccin Latte")
        );
        assert_eq!(
            theme_name_for_appearance("test", Appearance::Dark),
            Some("test")
        );
    }

    #[test]
    fn dark_theme_file_overrides_cli_resolved_light_selection_colors() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("test"),
            r#"
palette = 1=#a71f00
background = #131313
foreground = #ffffff
selection-background = #1f0f02
selection-foreground = #f0d77d
"#,
        )
        .unwrap();
        let mut theme = parse_ghostty_config(
            r#"
theme = light:Catppuccin Latte,dark:test
background = #eff1f5
foreground = #4c4f69
selection-foreground = #4c4f69
selection-background = #acb0be
background-image = /Users/heyglassy/.config/ghostty/backgrounds/insignia.png
"#,
        );

        assert!(
            apply_theme_file_for_appearance_from_dirs(
                &mut theme,
                Appearance::Dark,
                &[dir.path().to_path_buf()]
            )
            .unwrap()
            .is_some()
        );

        assert_eq!(
            theme.selection_background,
            RgbColor {
                r: 0x1f,
                g: 0x0f,
                b: 0x02
            }
        );
        assert_eq!(
            theme.selection_foreground,
            RgbColor {
                r: 0xf0,
                g: 0xd7,
                b: 0x7d
            }
        );
        assert_eq!(
            theme.palette[1],
            RgbColor {
                r: 0xa7,
                g: 0x1f,
                b: 0x00
            }
        );
        assert_eq!(
            theme.background_image.as_deref(),
            Some("/Users/heyglassy/.config/ghostty/backgrounds/insignia.png")
        );
    }

    #[test]
    fn status_style_uses_resolved_ghostty_selection_colors() {
        let theme = parse_ghostty_config(
            r#"
selection-foreground = #4c4f69
selection-background = #acb0be
"#,
        );
        let style = theme.status_style();

        assert_eq!(
            style.foreground,
            RgbColor {
                r: 0x4c,
                g: 0x4f,
                b: 0x69
            }
        );
        assert_eq!(
            style.background,
            RgbColor {
                r: 0xac,
                g: 0xb0,
                b: 0xbe
            }
        );
    }

    #[test]
    fn display_pwd_decodes_file_uri_paths() {
        assert_eq!(
            display_pwd("file://localhost/tmp/insignia%20project"),
            "/tmp/insignia project"
        );
        assert_eq!(
            display_pwd("file://insignia/tmp/insignia%20project"),
            "/tmp/insignia project"
        );
        assert_eq!(
            display_pwd("file:///tmp/insignia%20project"),
            "/tmp/insignia project"
        );
        assert_eq!(display_pwd("/tmp/plain"), "/tmp/plain");
    }

    #[test]
    fn outer_terminal_pwd_sequence_forwards_osc7_file_uri() {
        assert_eq!(
            outer_terminal_pwd_sequence("file://localhost/tmp/insignia%20project").as_deref(),
            Some("\x1b]7;file://localhost/tmp/insignia%20project\x1b\\")
        );
        assert_eq!(outer_terminal_pwd_sequence("/tmp/plain"), None);
        assert_eq!(
            outer_terminal_pwd_sequence("file://localhost/tmp/\x1b]0;bad"),
            None
        );
    }

    #[test]
    fn resolves_simple_cd_commands_for_status_fallback() {
        assert_eq!(
            resolve_cd_command("cd /tmp", "/Users/heyglassy/Documents/New project 5"),
            Some("/tmp".to_string())
        );
        assert_eq!(
            resolve_cd_command("cd '/tmp'", "/Users/heyglassy/Documents/New project 5"),
            Some("/tmp".to_string())
        );
        assert_eq!(
            resolve_cd_command("cd /definitely/not/a/real/path", "/tmp"),
            None
        );
    }

    #[test]
    fn shifted_character_keys_map_to_physical_base_key() {
        assert_eq!(
            crossterm_key_to_ghostty(KeyCode::Char('!')),
            Some(ghostty_key::Key::Digit1)
        );
        assert_eq!(unshifted_char('?'), '/');
    }

    #[test]
    fn function_key_mapping_supports_editor_keys() {
        assert_eq!(
            crossterm_key_to_ghostty(KeyCode::F(12)),
            Some(ghostty_key::Key::F12)
        );
        assert_eq!(crossterm_key_to_ghostty(KeyCode::F(26)), None);
    }

    #[test]
    fn cursor_rendering_paints_the_cursor_cell() {
        let area = Rect::new(0, 0, 10, 4);
        let mut buf = Buffer::empty(area);
        render_cursor(
            RenderCursor {
                pos: CellPos { col: 2, row: 1 },
                color: RgbColor {
                    r: 0xf5,
                    g: 0xcc,
                    b: 0x36,
                },
                text: RgbColor {
                    r: 0x50,
                    g: 0x50,
                    b: 0x50,
                },
                style: CursorVisualStyle::Block,
            },
            area,
            &mut buf,
        );

        let cell = &buf[(2, 1)];
        assert_eq!(cell.fg, Color::Rgb(0x50, 0x50, 0x50));
        assert_eq!(cell.bg, Color::Rgb(0xf5, 0xcc, 0x36));
    }

    #[test]
    fn cell_range_normalizes_drag_direction() {
        let range = CellRange::new(CellPos { col: 4, row: 2 }, CellPos { col: 1, row: 1 });

        assert_eq!(range.start, CellPos { col: 1, row: 1 });
        assert_eq!(range.end, CellPos { col: 4, row: 2 });
        assert!(range.contains(CellPos { col: 3, row: 1 }));
        assert!(range.contains(CellPos { col: 0, row: 2 }));
        assert!(!range.contains(CellPos { col: 0, row: 1 }));
    }

    #[test]
    fn mouse_coordinates_are_relative_to_terminal_area() {
        let area = Rect::new(10, 5, 80, 20);
        let mouse = MouseEvent {
            kind: MouseEventKind::Moved,
            column: 12,
            row: 8,
            modifiers: KeyModifiers::NONE,
        };

        assert_eq!(mouse_to_cell(mouse, area), Some(CellPos { col: 2, row: 3 }));
    }

    #[test]
    fn drag_mouse_events_create_selection_range() {
        let area = Rect::new(0, 0, 80, 20);
        let mut overlay = InteractionOverlay::default();

        assert!(overlay.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 3,
                row: 2,
                modifiers: KeyModifiers::NONE,
            },
            area,
        ));
        assert!(overlay.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: 6,
                row: 3,
                modifiers: KeyModifiers::NONE,
            },
            area,
        ));
        assert!(overlay.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 6,
                row: 3,
                modifiers: KeyModifiers::NONE,
            },
            area,
        ));

        assert!(!overlay.selecting);
        assert_eq!(
            overlay.selected_range(),
            Some(CellRange {
                start: CellPos { col: 3, row: 2 },
                end: CellPos { col: 6, row: 3 },
            })
        );
    }

    #[test]
    fn key_press_and_repeat_reset_interaction_overlay() {
        assert!(key_resets_overlay(KeyEvent::new_with_kind(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )));
        assert!(key_resets_overlay(KeyEvent::new_with_kind(
            KeyCode::Down,
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )));
        assert!(!key_resets_overlay(KeyEvent::new_with_kind(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        )));
    }

    #[test]
    fn mouse_hover_does_not_repaint_over_active_selection() {
        let area = Rect::new(0, 0, 80, 20);
        let mut overlay = InteractionOverlay::default();
        overlay.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 3,
                row: 2,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        overlay.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 6,
                row: 2,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );

        assert!(!overlay.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Moved,
                column: 7,
                row: 2,
                modifiers: KeyModifiers::NONE,
            },
            area,
        ));
        assert_eq!(overlay.hover, None);
        assert_eq!(
            overlay.selected_range(),
            Some(CellRange {
                start: CellPos { col: 3, row: 2 },
                end: CellPos { col: 6, row: 2 },
            })
        );
    }

    #[test]
    fn interaction_style_uses_selection_colors() {
        let theme = parse_ghostty_config(
            r#"
background = #eff1f5
selection-foreground = #4c4f69
selection-background = #acb0be
"#,
        );
        let style = theme.interaction_style();

        assert_eq!(
            style.foreground,
            RgbColor {
                r: 0x4c,
                g: 0x4f,
                b: 0x69
            }
        );
        assert_eq!(
            style.background,
            RgbColor {
                r: 0xac,
                g: 0xb0,
                b: 0xbe
            }
        );
        assert_eq!(
            style.hover_background,
            RgbColor {
                r: 0xd8,
                g: 0xda,
                b: 0xe2
            }
        );
    }

    #[test]
    fn default_terminal_background_resets_to_host_surface() {
        let style = terminal_cell_style(Color::White, None);

        assert_eq!(style.bg, Some(Color::Reset));
    }

    #[test]
    fn explicit_terminal_background_is_preserved() {
        let style = terminal_cell_style(Color::White, Some(Color::Blue));

        assert_eq!(style.bg, Some(Color::Blue));
    }
}
