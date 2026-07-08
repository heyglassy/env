use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use libghostty_vt::{
    Terminal, TerminalOptions,
    render::{CellIterator, RenderState, RowIterator},
};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

const COLS: u16 = 100;
const ROWS: u16 = 24;

#[test]
fn draws_pty_content_and_bottom_status_bar() {
    let bin = env!("CARGO_BIN_EXE_glassterm");
    let shell = test_shell();
    let mut harness = TerminalHarness::spawn(
        bin,
        &["--", &shell, "-lc", "printf 'ready from pty'; sleep 30"],
        &[
            ("GLASSTERM_DISABLE_THEME_QUERY", "1"),
            ("GLASSTERM_TEST_TIME", "19:17"),
            ("GLASSTERM_STATUS_LABEL", "eulogia"),
            (
                "GLASSTERM_STATUS_CWD",
                "/Users/heyglassy/.config/ghostty/themes",
            ),
        ],
    );

    let snapshot = harness.wait_for_text("ready from pty", Duration::from_secs(5));
    snapshot.record("status_bar");

    assert!(
        snapshot.text().contains("ready from pty"),
        "snapshot did not include child output:\n{}",
        snapshot.text()
    );
    assert_eq!(
        snapshot.line(ROWS - 1).trim(),
        "eulogia  |  /Users/heyglassy/.config/ghostty/themes  |  19:17"
    );

    harness.send(b"\x11");
}

#[test]
fn resize_churn_repaints_full_status_bar() {
    let bin = env!("CARGO_BIN_EXE_glassterm");
    let shell = test_shell();
    // Label is deliberately distinct from the cwd so a "no stale bar above"
    // check on the label cannot false-match cwd text.
    let mut harness = TerminalHarness::spawn(
        bin,
        &["--", &shell, "-lc", "printf 'ready from pty'; sleep 30"],
        &[
            ("GLASSTERM_DISABLE_THEME_QUERY", "1"),
            ("GLASSTERM_TEST_TIME", "19:17"),
            ("GLASSTERM_STATUS_LABEL", "eulogia"),
            ("GLASSTERM_STATUS_CWD", "/private/tmp/work"),
        ],
    );

    let expected = "eulogia  |  /private/tmp/work  |  19:17";

    let snapshot = harness.wait_for_text("ready from pty", Duration::from_secs(5));
    assert_eq!(
        snapshot.line(ROWS - 1).trim(),
        expected,
        "baseline status bar not drawn on bottom row:\n{}",
        snapshot.text()
    );

    // Transient host churn: ROWS -> ROWS-1 -> ROWS, delivered back to back so
    // both SIGWINCHes land well within one glassterm frame (FRAME_TIME = 16ms)
    // and the net size is unchanged. This mirrors a window being restored at
    // terminal restart: the host shifts screen content by a row, but glassterm's
    // fullscreen autoresize sees an unchanged size and never clears/full-repaints.
    harness.resize(COLS, ROWS - 1);
    harness.resize(COLS, ROWS);

    // Let glassterm react and redraw, draining its output into the vt.
    let snapshot = harness.pump(Duration::from_millis(400));
    snapshot.record("resize_churn_status_bar");

    assert_eq!(
        snapshot.line(ROWS - 1).trim(),
        expected,
        "bottom row must show the complete status bar after resize churn:\n{}",
        snapshot.text()
    );

    for row in 0..ROWS - 1 {
        assert!(
            !snapshot.line(row).contains("eulogia"),
            "stale status bar found on row {row} after resize churn:\n{}",
            snapshot.text()
        );
    }

    harness.send(b"\x11");
}

fn test_shell() -> String {
    std::env::var("GLASSTERM_TEST_SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
}

fn wait_for_file_contains(path: &Path, needle: &str, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if let Ok(contents) = fs::read_to_string(path)
            && contents.contains(needle)
        {
            return true;
        }
        thread::sleep(Duration::from_millis(25));
    }
    false
}

/// Cmd+left-click on a URL opens it. As it reaches glassterm, a macOS Cmd+click
/// is a Kitty-protocol Super keydown (CSI u) plus a plain SGR mouse report with
/// no Super bit (Ghostty strips it — the SGR protocol has no Super bit, and
/// glassterm always presents mouse-reporting-on to Ghostty). glassterm must
/// recognize the click via its keyboard-derived Super-held state.
#[test]
fn cmd_click_opens_url_under_pointer() {
    let bin = env!("CARGO_BIN_EXE_glassterm");
    let shell = test_shell();

    let mut open_url_file = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    open_url_file.push("cmd_click_opened_urls.txt");
    let _ = fs::remove_file(&open_url_file);
    let open_url_file_str = open_url_file.to_str().expect("utf8 path");

    let mut harness = TerminalHarness::spawn(
        bin,
        &[
            "--",
            &shell,
            "-lc",
            "printf 'open https://example.test/path '; sleep 30",
        ],
        &[
            ("GLASSTERM_DISABLE_THEME_QUERY", "1"),
            ("GLASSTERM_TEST_TIME", "19:17"),
            ("GLASSTERM_TEST_OPEN_URL_FILE", open_url_file_str),
        ],
    );

    let snapshot = harness.wait_for_text("https://example.test/path", Duration::from_secs(5));
    assert!(
        snapshot.text().contains("https://example.test/path"),
        "child URL not visible before click:\n{}",
        snapshot.text()
    );

    // "open " occupies columns 1..=5, so the URL begins at 1-based column 6;
    // SGR column 10 (0-based 9) lands inside the URL. Row 1 is the top viewport
    // row where the child printed.
    harness.write_input(b"\x1b[57444;9:1u"); // LeftSuper press (Kitty CSI u)
    harness.write_input(b"\x1b[<0;10;1M"); // left button down at col 10, row 1
    harness.write_input(b"\x1b[<0;10;1m"); // left button up
    harness.write_input(b"\x1b[57444;9:3u"); // LeftSuper release (Kitty CSI u)

    let opened = wait_for_file_contains(
        &open_url_file,
        "https://example.test/path",
        Duration::from_secs(3),
    );
    assert!(
        opened,
        "Cmd+click did not open the URL; file contents: {:?}",
        fs::read_to_string(&open_url_file).ok()
    );

    harness.send(b"\x11");
}

#[test]
fn osc7_updates_bottom_status_cwd() {
    let bin = env!("CARGO_BIN_EXE_glassterm");
    let shell = test_shell();
    let mut harness = TerminalHarness::spawn(
        bin,
        &[
            "--",
            &shell,
            "-lc",
            "printf '\\033]7;file://localhost/tmp/glassterm%%20project\\033\\\\'; printf 'ready after osc7'; sleep 30",
        ],
        &[
            ("GLASSTERM_DISABLE_THEME_QUERY", "1"),
            ("GLASSTERM_TEST_TIME", "19:17"),
            ("GLASSTERM_STATUS_CWD", "/tmp/old"),
        ],
    );

    let snapshot = harness.wait_for_text("ready after osc7", Duration::from_secs(5));
    snapshot.record("osc7_status_bar");

    assert_eq!(
        snapshot.line(ROWS - 1).trim(),
        "glassterm  |  /tmp/glassterm project  |  19:17"
    );

    harness.send(b"\x11");
}

struct TerminalHarness {
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    output_rx: mpsc::Receiver<Vec<u8>>,
    vt: Terminal<'static, 'static>,
}

impl TerminalHarness {
    fn spawn(program: &str, args: &[&str], envs: &[(&str, &str)]) -> Self {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: ROWS,
                cols: COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("open pty");

        let mut command = CommandBuilder::new(program);
        for arg in args {
            command.arg(arg);
        }
        command.env("TERM", "xterm-256color");
        for (key, value) in envs {
            command.env(key, value);
        }

        let child = pair.slave.spawn_command(command).expect("spawn command");
        drop(pair.slave);
        let writer = pair.master.take_writer().expect("take writer");
        let mut reader = pair.master.try_clone_reader().expect("clone pty reader");
        let (output_tx, output_rx) = mpsc::channel();

        thread::Builder::new()
            .name("terminal-harness-reader".to_string())
            .spawn(move || {
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
            })
            .expect("spawn pty reader");

        Self {
            master: pair.master,
            writer,
            child,
            output_rx,
            vt: Terminal::new(TerminalOptions {
                cols: COLS,
                rows: ROWS,
                max_scrollback: 1000,
            })
            .expect("create vt"),
        }
    }

    fn wait_for_text(&mut self, needle: &str, timeout: Duration) -> TerminalSnapshot {
        let started = Instant::now();

        while started.elapsed() < timeout {
            let remaining = timeout.saturating_sub(started.elapsed());
            match self
                .output_rx
                .recv_timeout(remaining.min(Duration::from_millis(50)))
            {
                Ok(bytes) => self.vt.vt_write(&bytes),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }

            let snapshot = TerminalSnapshot::from_vt(&mut self.vt, COLS, ROWS);
            if snapshot.text().contains(needle) {
                return snapshot;
            }
        }

        TerminalSnapshot::from_vt(&mut self.vt, COLS, ROWS)
    }

    /// Resize the master pty (delivers SIGWINCH to glassterm) and mirror the
    /// same resize onto our vt, which emulates the host terminal's outer-screen
    /// reflow/scroll behavior.
    fn resize(&mut self, cols: u16, rows: u16) {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("resize pty");
        self.vt.resize(cols, rows, 0, 0).expect("resize vt");
    }

    /// Drain glassterm output into the vt for the given duration, then snapshot.
    fn pump(&mut self, duration: Duration) -> TerminalSnapshot {
        let started = Instant::now();
        while started.elapsed() < duration {
            let remaining = duration.saturating_sub(started.elapsed());
            match self
                .output_rx
                .recv_timeout(remaining.min(Duration::from_millis(20)))
            {
                Ok(bytes) => self.vt.vt_write(&bytes),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        TerminalSnapshot::from_vt(&mut self.vt, COLS, ROWS)
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).expect("write input");
        self.writer.flush().expect("flush input");
        thread::sleep(Duration::from_millis(50));
        let _ = self.child.kill();
    }

    /// Write bytes to glassterm's stdin without killing the child, so a test can
    /// drive input and then keep observing glassterm's behavior.
    fn write_input(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).expect("write input");
        self.writer.flush().expect("flush input");
    }
}

impl Drop for TerminalHarness {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

struct TerminalSnapshot {
    lines: Vec<String>,
}

impl TerminalSnapshot {
    fn from_vt(vt: &mut Terminal<'_, '_>, cols: u16, rows: u16) -> Self {
        let mut state = RenderState::new().expect("render state");
        let snapshot = state.update(vt).expect("snapshot");
        let mut row_iter = RowIterator::new().expect("row iterator");
        let mut cell_iter = CellIterator::new().expect("cell iterator");
        let mut rendered = row_iter.update(&snapshot).expect("rows");
        let mut lines = Vec::new();

        while let Some(row) = rendered.next() {
            if lines.len() >= rows as usize {
                break;
            }

            let mut cells = cell_iter.update(row).expect("cells");
            let mut line = String::new();
            while let Some(cell) = cells.next() {
                if line.chars().count() >= cols as usize {
                    break;
                }
                match cell.graphemes() {
                    Ok(graphemes) if !graphemes.is_empty() => {
                        line.extend(graphemes.into_iter());
                    }
                    _ => line.push(' '),
                }
            }
            while line.chars().count() < cols as usize {
                line.push(' ');
            }
            lines.push(line);
        }

        while lines.len() < rows as usize {
            lines.push(" ".repeat(cols as usize));
        }

        Self { lines }
    }

    fn line(&self, row: u16) -> &str {
        &self.lines[row as usize]
    }

    fn text(&self) -> String {
        self.lines.join("\n")
    }

    fn record(&self, name: &str) {
        let mut path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        path.push(format!("{name}.snapshot.txt"));
        fs::write(&path, self.text()).expect("write snapshot");
        eprintln!("recorded terminal snapshot at {}", path.display());
    }
}
