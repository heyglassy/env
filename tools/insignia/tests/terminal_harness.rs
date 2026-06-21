use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
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
    let bin = env!("CARGO_BIN_EXE_insignia");
    let shell = test_shell();
    let mut harness = TerminalHarness::spawn(
        bin,
        &["--", &shell, "-lc", "printf 'ready from pty'; sleep 30"],
        &[
            ("INSIGNIA_DISABLE_THEME_QUERY", "1"),
            ("INSIGNIA_TEST_TIME", "19:17"),
            (
                "INSIGNIA_STATUS_CWD",
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
        "insignia  |  /Users/heyglassy/.config/ghostty/themes  |  19:17"
    );

    harness.send(b"\x11");
}

fn test_shell() -> String {
    std::env::var("INSIGNIA_TEST_SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
}

#[test]
fn osc7_updates_bottom_status_cwd() {
    let bin = env!("CARGO_BIN_EXE_insignia");
    let shell = test_shell();
    let mut harness = TerminalHarness::spawn(
        bin,
        &[
            "--",
            &shell,
            "-lc",
            "printf '\\033]7;file://localhost/tmp/insignia%%20project\\033\\\\'; printf 'ready after osc7'; sleep 30",
        ],
        &[
            ("INSIGNIA_DISABLE_THEME_QUERY", "1"),
            ("INSIGNIA_TEST_TIME", "19:17"),
            ("INSIGNIA_STATUS_CWD", "/tmp/old"),
        ],
    );

    let snapshot = harness.wait_for_text("ready after osc7", Duration::from_secs(5));
    snapshot.record("osc7_status_bar");

    assert_eq!(
        snapshot.line(ROWS - 1).trim(),
        "insignia  |  /tmp/insignia project  |  19:17"
    );

    harness.send(b"\x11");
}

struct TerminalHarness {
    _master: Box<dyn portable_pty::MasterPty + Send>,
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
            _master: pair.master,
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

    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).expect("write input");
        self.writer.flush().expect("flush input");
        thread::sleep(Duration::from_millis(50));
        let _ = self.child.kill();
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
