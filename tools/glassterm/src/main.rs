mod clipboard;
mod config;
mod input;
mod overlay;
mod pty;
mod render;
mod search;
mod selection;
mod status;
mod theme;
mod urls;

use std::{
    env, fs,
    io::{self, Write},
    path::PathBuf,
    process,
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::{
    cursor::{self, SetCursorStyle},
    event::{
        self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, KeyboardEnhancementFlags, MouseButton, MouseEvent, MouseEventKind,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{self, Clear, ClearType},
};
use libghostty_vt::{
    Terminal, TerminalOptions, focus as ghostty_focus, key as ghostty_key, mouse as ghostty_mouse,
    render::RenderState,
    terminal::{Mode, ScrollViewport},
};
use ratatui::{Terminal as RatatuiTerminal, backend::CrosstermBackend, layout::Rect};
use tracing::{debug, info, warn};
use tracing_appender::non_blocking::WorkerGuard;

use crate::{
    clipboard::write_clipboard,
    config::Config,
    input::{
        SuperKeyState, encode_focus_event, encode_key_event, encode_mouse_event, encode_paste,
    },
    pty::PtySession,
    render::{GhosttyRenderer, mouse_to_cell},
    search::SearchState,
    selection::{MouseSelection, is_left_button_event, key_clears_selection},
    status::{ShellInputTracker, StatusBar, display_pwd, notify_outer_terminal_pwd},
    theme::{Theme, format_rgb},
    urls::{open_url, url_at_viewport_cell},
};

const FRAME_TIME: Duration = Duration::from_millis(16);
const EVENT_POLL: Duration = Duration::from_millis(33);
const SCROLL_WHEEL_LINES: isize = 3;
static TERMINAL_RESTORED: AtomicBool = AtomicBool::new(true);

fn main() -> Result<()> {
    let _trace_guard = init_tracing();
    let config = Config::load();
    let command = command_from_args();
    info!(?command, "starting glassterm");
    let mut app = TerminalApp::new(command, config)?;
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
    let enabled = env::var("GLASSTERM_TRACE")
        .ok()
        .is_some_and(|value| !matches!(value.as_str(), "" | "0" | "false" | "off"));
    if !enabled {
        return None;
    }

    let log_dir = env::var_os("GLASSTERM_TRACE_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Logs/glassterm"))
        })
        .unwrap_or_else(|| env::temp_dir().join("glassterm"));

    if let Err(error) = fs::create_dir_all(&log_dir) {
        eprintln!(
            "glassterm: failed to create trace log directory {}: {error}",
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
        eprintln!("glassterm: failed to initialize tracing: {error}");
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
    config: Config,
    status: StatusBar,
    theme: Theme,
    input_tracker: ShellInputTracker,
}

impl TerminalApp {
    fn new(command: Vec<String>, config: Config) -> Result<Self> {
        Ok(Self {
            command,
            status: StatusBar::new(&config)?,
            theme: Theme::default(),
            input_tracker: ShellInputTracker::default(),
            config,
        })
    }

    fn run(&mut self) -> Result<()> {
        let _guard = TerminalGuard::enter()?;
        self.theme = match Theme::query(&self.config) {
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
        self.status
            .set_theme(self.theme.status_style(), &self.config);
        let interaction = self.theme.interaction_style();
        let selection_fg = interaction.foreground;
        let selection_bg = interaction.background;

        let mut stdout = io::stdout();
        execute!(
            stdout,
            Clear(ClearType::All),
            cursor::MoveTo(0, 0),
            EnableFocusChange,
            EnableBracketedPaste,
            // Keep mouse capture on for the whole session so glassterm receives
            // wheel events for its own scrollback even when the child program
            // has not requested mouse tracking. restore_terminal disables it.
            EnableMouseCapture,
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
        let mut session =
            PtySession::spawn(&self.command, initial.width, initial_pty_rows, &self.config)?;

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
        let mut selection = MouseSelection::new()?;
        let mut search = SearchState::new();
        let mut key_encoder = ghostty_key::Encoder::new()?;
        let mut mouse_encoder = ghostty_mouse::Encoder::new()?;
        let mut last_frame = Instant::now()
            .checked_sub(FRAME_TIME)
            .unwrap_or_else(Instant::now);
        let mut dirty = true;
        let mut hover_cell = None;
        let mut hover_url = None;
        let mut hover_dirty = true;
        let mut running = true;
        let mut suppress_url_click_until_up = false;
        // SGR mouse reports carry no Super/Cmd bit, so Cmd+click arrives as a
        // plain left-click. Track Super's held state from Kitty keyboard events
        // instead and consult it when classifying clicks (see SuperKeyState).
        let mut super_key = SuperKeyState::default();

        while running {
            let mut got_output = false;
            while let Ok(bytes) = session.output_rx.try_recv() {
                vt.vt_write(&bytes);
                dirty = true;
                got_output = true;
            }
            if got_output {
                // Content changed: any active search's absolute-row matches may
                // now be stale (see SearchState::note_content_changed). Hover
                // underline is intentionally NOT invalidated here — it is
                // recomputed on pointer movement, scroll, and resize instead.
                search.note_content_changed();
            }

            if dirty {
                let now = Instant::now();
                if now.duration_since(last_frame) >= FRAME_TIME {
                    if hover_dirty {
                        hover_url = hover_cell.and_then(|cell| url_at_viewport_cell(&vt, cell));
                        hover_dirty = false;
                    }
                    terminal.draw(|frame| {
                        let area = frame.area();
                        let body = terminal_area(area);
                        renderer.render(
                            &mut vt,
                            &mut render_state,
                            body,
                            frame.buffer_mut(),
                            self.theme.cursor_text,
                            selection_fg,
                            selection_bg,
                            hover_url.as_ref(),
                        );
                        if search.is_active() {
                            self.status
                                .render_prompt(area, frame.buffer_mut(), &search.prompt());
                        } else {
                            self.status.render(area, frame.buffer_mut());
                        }
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
                        // Keep the Super-held tracker current for every key event
                        // (the Super keypress itself is still forwarded to the
                        // child below, unchanged).
                        super_key.handle_key(key);

                        if should_quit(key) {
                            break;
                        }

                        if is_search_toggle(key) {
                            // Opening search: drop any mouse selection so its
                            // `active` flag can't linger stale-true across the
                            // search session and later swallow an Esc meant for
                            // the child.
                            if !search.is_active() {
                                selection.clear(&vt);
                            }
                            search.toggle(&mut vt);
                            dirty = true;
                            continue;
                        }

                        // While search is active it owns all key input: the
                        // query is edited here and never forwarded to the child.
                        if search.is_active() {
                            if search.handle_key(&mut vt, key) {
                                dirty = true;
                            }
                            continue;
                        }

                        if self.input_tracker.handle_key(key, &self.status) {
                            dirty = true;
                        }

                        let selection_cleared = key_clears_selection(key) && selection.clear(&vt);
                        if selection_cleared {
                            dirty = true;
                        }

                        if key.kind == KeyEventKind::Press
                            && key.code == KeyCode::Esc
                            && selection_cleared
                        {
                            continue;
                        }

                        if handle_scrollback_key(
                            &mut vt,
                            key,
                            terminal_area(terminal.size()?.into()),
                        ) {
                            dirty = true;
                            continue;
                        }

                        if let Some(encoded) = encode_key_event(&mut key_encoder, &vt, key)? {
                            session.write_all(&encoded)?;
                        }
                    }
                    Event::Mouse(mouse) => {
                        let area = terminal_area(terminal.size()?.into());
                        let next_hover_cell = mouse_to_cell(mouse, area);
                        if next_hover_cell != hover_cell {
                            hover_cell = next_hover_cell;
                            hover_dirty = true;
                            dirty = true;
                        }
                        let tracking = vt.is_mouse_tracking().unwrap_or(false);
                        if suppress_url_click_until_up && is_left_button_event(mouse.kind) {
                            if matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left)) {
                                suppress_url_click_until_up = false;
                            }
                            continue;
                        }
                        // The SUPER modifier flag is never set on SGR mouse
                        // reports (the protocol lacks a Super bit), so fall back
                        // to the keyboard-derived held state for Cmd+click.
                        let cmd_left = (mouse.modifiers.contains(KeyModifiers::SUPER)
                            || super_key.is_held())
                            && is_left_button_event(mouse.kind);
                        if cmd_left && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                        {
                            if let Some(cell) = next_hover_cell
                                && let Some(found) = url_at_viewport_cell(&vt, cell)
                            {
                                open_url(&found.url);
                                hover_url = Some(found);
                                hover_dirty = false;
                                dirty = true;
                                suppress_url_click_until_up = true;
                                continue;
                            }
                            if !tracking {
                                suppress_url_click_until_up = true;
                                continue;
                            }
                        }
                        // Ghostty convention: Shift+left-drag selects even while
                        // the child has mouse tracking enabled.
                        let shift_select = tracking
                            && mouse.modifiers.contains(KeyModifiers::SHIFT)
                            && is_left_button_event(mouse.kind);
                        if tracking && !shift_select {
                            if let Some(encoded) =
                                encode_mouse_event(&mut mouse_encoder, &vt, mouse, area)?
                            {
                                session.write_all(&encoded)?;
                            }
                        } else if handle_scrollback_mouse(&mut vt, mouse, area) {
                            hover_dirty = true;
                            dirty = true;
                        } else if cmd_left {
                            if matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left)) {
                                suppress_url_click_until_up = false;
                            }
                        } else if !search.is_active() {
                            // While the search prompt owns the highlight, mouse
                            // drags must not clobber it or desync the (n/m)
                            // count. Scrollback wheel and Cmd+click above stay
                            // live; only glassterm's own drag-select is gated.
                            let outcome = selection.handle_mouse(&vt, mouse, area);
                            if outcome.changed {
                                dirty = true;
                            }
                            if let Some(text) = outcome.copy {
                                write_clipboard(&text);
                            }
                        }
                    }
                    Event::Paste(text) => {
                        let bracketed = vt.mode(Mode::BRACKETED_PASTE).unwrap_or(false);
                        session.write_all(&encode_paste(&text, bracketed))?;
                    }
                    Event::FocusGained => {
                        // A release for a held Super may never arrive across a
                        // focus change (e.g. Cmd+Tab away), so drop stale state.
                        super_key.clear();
                        if vt.mode(Mode::FOCUS_EVENT).unwrap_or(false) {
                            session
                                .write_all(&encode_focus_event(ghostty_focus::Event::Gained)?)?;
                        }
                    }
                    Event::FocusLost => {
                        super_key.clear();
                        if vt.mode(Mode::FOCUS_EVENT).unwrap_or(false) {
                            session.write_all(&encode_focus_event(ghostty_focus::Event::Lost)?)?;
                        }
                    }
                    Event::Resize(cols, rows) => {
                        // crossterm/ratatui cannot observe host-screen content
                        // shifts, and a transient A -> B -> A resize (common when
                        // a window is restored at terminal restart) leaves
                        // ratatui's last-known size unchanged, so its fullscreen
                        // autoresize never clears. Meanwhile the host terminal has
                        // scrolled our primary-screen content by a row, leaving a
                        // stale status bar behind. Force a full resync on every
                        // resize event: Terminal::resize clears the backend and
                        // resets the back buffer, so the next draw repaints every
                        // cell. (We deliberately avoid Terminal::clear here: it
                        // issues a blocking DSR cursor-position query that would
                        // stall the event loop waiting on the host to answer.)
                        let area = terminal.size()?.into();
                        terminal.resize(area)?;
                        let rows = pty_rows(rows);
                        session.resize(cols, rows)?;
                        vt.resize(cols, rows, 0, 0)?;
                        // Reflow invalidates every absolute match row; recompute
                        // the active search highlight against the new layout.
                        search.on_resize(&mut vt);
                        selection.clear(&vt);
                        hover_cell = None;
                        hover_url = None;
                        hover_dirty = true;
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

/// Cmd+F (Super+f), delivered via the Kitty keyboard protocol, toggles search.
fn is_search_toggle(key: KeyEvent) -> bool {
    key.kind == KeyEventKind::Press
        && key.code == KeyCode::Char('f')
        && key.modifiers.contains(KeyModifiers::SUPER)
}

fn handle_scrollback_key(vt: &mut Terminal<'_, '_>, key: KeyEvent, area: Rect) -> bool {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return false;
    }

    let page = area.height.saturating_sub(1).max(1) as isize;
    let scroll = if key.modifiers == KeyModifiers::SHIFT {
        match key.code {
            KeyCode::PageUp => Some(ScrollViewport::Delta(-page)),
            KeyCode::PageDown => Some(ScrollViewport::Delta(page)),
            KeyCode::Home => Some(ScrollViewport::Top),
            KeyCode::End => Some(ScrollViewport::Bottom),
            _ => None,
        }
    } else if key.modifiers == KeyModifiers::SUPER {
        match key.code {
            KeyCode::Up => Some(ScrollViewport::Top),
            KeyCode::Down => Some(ScrollViewport::Bottom),
            _ => None,
        }
    } else {
        None
    };

    let Some(scroll) = scroll else {
        return false;
    };

    vt.scroll_viewport(scroll);
    true
}

fn handle_scrollback_mouse(vt: &mut Terminal<'_, '_>, mouse: MouseEvent, area: Rect) -> bool {
    if mouse.column < area.left()
        || mouse.column >= area.right()
        || mouse.row < area.top()
        || mouse.row >= area.bottom()
    {
        return false;
    }

    let scroll = match mouse.kind {
        MouseEventKind::ScrollUp => Some(ScrollViewport::Delta(-SCROLL_WHEEL_LINES)),
        MouseEventKind::ScrollDown => Some(ScrollViewport::Delta(SCROLL_WHEEL_LINES)),
        _ => None,
    };

    let Some(scroll) = scroll else {
        return false;
    };

    vt.scroll_viewport(scroll);
    true
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
