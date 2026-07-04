use std::{
    cell::RefCell,
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use anyhow::Result;
use chrono::Local;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};
use tracing::warn;

use crate::{
    config::{Config, StatusSegment},
    render::rgb,
    theme::Theme,
};

const GIT_CACHE_TTL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy)]
pub(crate) struct StatusStyle {
    pub(crate) foreground: RgbColor,
    pub(crate) background: RgbColor,
}

use libghostty_vt::style::RgbColor;

impl Default for StatusStyle {
    fn default() -> Self {
        Theme::default().status_style()
    }
}

pub(crate) struct StatusBar {
    label: String,
    pub(crate) cwd: Rc<RefCell<String>>,
    style: StatusStyle,
    segments: Vec<StatusSegment>,
    time_format: String,
    git_cache: RefCell<GitCache>,
}

impl StatusBar {
    pub(crate) fn new(config: &Config) -> Result<Self> {
        let cwd = config.status_cwd.clone().unwrap_or_else(|| {
            env::current_dir()
                .ok()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "?".to_string())
        });
        Ok(Self {
            label: config.status_label.clone(),
            cwd: Rc::new(RefCell::new(cwd)),
            style: StatusStyle::default(),
            segments: config.status_segments.clone(),
            time_format: config.time_format.clone(),
            git_cache: RefCell::new(GitCache::default()),
        })
    }

    pub(crate) fn set_theme(&mut self, mut style: StatusStyle, config: &Config) {
        if let Some(foreground) = config.status_foreground {
            style.foreground = foreground;
        }
        if let Some(background) = config.status_background {
            style.background = background;
        }
        self.style = style;
    }

    pub(crate) fn render(&self, area: Rect, buf: &mut Buffer) {
        let cwd = self.cwd.borrow();
        let parts = self.rendered_segments(&cwd);
        let text = format!(" {} ", parts.join("  |  "));
        self.paint(area, buf, &text);
    }

    /// Render arbitrary text (e.g. the search prompt) on the status row using
    /// the status bar's own style.
    pub(crate) fn render_prompt(&self, area: Rect, buf: &mut Buffer, text: &str) {
        self.paint(area, buf, text);
    }

    fn paint(&self, area: Rect, buf: &mut Buffer, text: &str) {
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

        write_clipped(buf, area.x, y, area.width, text, style);
    }

    fn rendered_segments(&self, cwd: &str) -> Vec<String> {
        let mut rendered = Vec::new();
        for segment in &self.segments {
            match segment {
                StatusSegment::Label => rendered.push(self.label.clone()),
                StatusSegment::Cwd => rendered.push(cwd.to_string()),
                StatusSegment::Git => {
                    if let Some(branch) = self.git_cache.borrow_mut().branch(cwd) {
                        rendered.push(branch);
                    }
                }
                StatusSegment::Time => rendered.push(status_time(&self.time_format)),
            }
        }
        rendered
    }
}

#[derive(Default)]
struct GitCache {
    cwd: String,
    branch: Option<String>,
    refreshed: Option<Instant>,
}

impl GitCache {
    fn branch(&mut self, cwd: &str) -> Option<String> {
        let fresh = self.cwd == cwd
            && self
                .refreshed
                .is_some_and(|refreshed| refreshed.elapsed() < GIT_CACHE_TTL);
        if !fresh {
            self.cwd = cwd.to_string();
            self.branch = git_branch_for_cwd(cwd);
            self.refreshed = Some(Instant::now());
        }
        self.branch.clone()
    }
}

fn status_time(time_format: &str) -> String {
    status_time_from_env(time_format, env::var("GLASSTERM_TEST_TIME").ok())
}

fn status_time_from_env(time_format: &str, test_time: Option<String>) -> String {
    test_time.unwrap_or_else(|| Local::now().format(time_format).to_string())
}

fn git_branch_for_cwd(cwd: &str) -> Option<String> {
    let git_dir = find_git_dir(Path::new(cwd))?;
    parse_git_head(&fs::read_to_string(git_dir.join("HEAD")).ok()?)
}

fn find_git_dir(cwd: &Path) -> Option<PathBuf> {
    for dir in cwd.ancestors() {
        let marker = dir.join(".git");
        if marker.is_dir() {
            return Some(marker);
        }
        if marker.is_file() {
            return read_gitdir_file(&marker, dir);
        }
    }
    None
}

fn read_gitdir_file(marker: &Path, worktree_dir: &Path) -> Option<PathBuf> {
    let contents = fs::read_to_string(marker).ok()?;
    let gitdir = contents.trim().strip_prefix("gitdir:")?.trim();
    let path = PathBuf::from(gitdir);
    Some(if path.is_absolute() {
        path
    } else {
        worktree_dir.join(path)
    })
}

fn parse_git_head(head: &str) -> Option<String> {
    let head = head.trim();
    if let Some(branch) = head.strip_prefix("ref: refs/heads/") {
        return (!branch.is_empty()).then(|| branch.to_string());
    }

    if head.len() >= 8 && head.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Some(head[..8].to_string())
    } else {
        None
    }
}

pub(crate) fn display_pwd(pwd: &str) -> String {
    let Some(path) = pwd.strip_prefix("file://") else {
        return pwd.to_string();
    };
    let path = path.find('/').map(|index| &path[index..]).unwrap_or(path);
    percent_decode(path)
}

pub(crate) fn notify_outer_terminal_pwd(pwd: &str) {
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
    let bytes = input.as_bytes();
    // Decode into raw bytes, then interpret the whole result as UTF-8. Pushing
    // each decoded byte straight into a String (as a `char`) treats it as
    // Latin-1 and mangles multi-byte sequences like "%C3%A9" ("é").
    let mut output: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Some(decoded) = decode_hex_byte(bytes[index + 1], bytes[index + 2])
        {
            output.push(decoded);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&output).into_owned()
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
    // Use ratatui's width-aware writer: a one-cell-per-char loop corrupts the
    // bar for double-width glyphs (CJK/emoji), writing the next char into the
    // wide glyph's spacer cell. set_stringn clips to `width` display columns and
    // lays out wide glyphs correctly.
    buf.set_stringn(x, y, text, width as usize, style);
}

#[derive(Default)]
pub(crate) struct ShellInputTracker {
    line: String,
}

impl ShellInputTracker {
    pub(crate) fn handle_key(&mut self, key: KeyEvent, status: &StatusBar) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_git_head_branch_and_detached_sha() {
        assert_eq!(
            parse_git_head("ref: refs/heads/feature/config\n"),
            Some("feature/config".to_string())
        );
        assert_eq!(
            parse_git_head("0123456789abcdef0123456789abcdef01234567\n"),
            Some("01234567".to_string())
        );
        assert_eq!(parse_git_head("ref: refs/tags/v1"), None);
    }

    #[test]
    fn resolves_git_branch_from_git_directory() {
        let repo = tempfile::tempdir().unwrap();
        let nested = repo.path().join("src/bin");
        fs::create_dir_all(&nested).unwrap();
        let git = repo.path().join(".git");
        fs::create_dir(&git).unwrap();
        fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();

        assert_eq!(
            git_branch_for_cwd(&nested.display().to_string()),
            Some("main".to_string())
        );
    }

    #[test]
    fn resolves_git_branch_from_gitdir_file() {
        let worktree = tempfile::tempdir().unwrap();
        let actual_git = worktree.path().join("actual.git");
        fs::create_dir(&actual_git).unwrap();
        fs::write(worktree.path().join(".git"), "gitdir: actual.git\n").unwrap();
        fs::write(actual_git.join("HEAD"), "ref: refs/heads/worktree\n").unwrap();

        assert_eq!(
            git_branch_for_cwd(&worktree.path().display().to_string()),
            Some("worktree".to_string())
        );
    }

    #[test]
    fn status_time_uses_test_time_override() {
        assert_eq!(
            status_time_from_env("%S", Some("19:17".to_string())),
            "19:17"
        );
    }

    #[test]
    fn display_pwd_decodes_file_uri_paths() {
        assert_eq!(
            display_pwd("file://localhost/tmp/glassterm%20project"),
            "/tmp/glassterm project"
        );
        assert_eq!(
            display_pwd("file://glassterm/tmp/glassterm%20project"),
            "/tmp/glassterm project"
        );
        assert_eq!(
            display_pwd("file:///tmp/glassterm%20project"),
            "/tmp/glassterm project"
        );
        assert_eq!(display_pwd("/tmp/plain"), "/tmp/plain");
    }

    #[test]
    fn percent_decode_reassembles_utf8_sequences() {
        // Regression: bytes were pushed as Latin-1 chars, mangling multi-byte
        // UTF-8. "%C3%A9" is the UTF-8 encoding of 'é'.
        assert_eq!(percent_decode("caf%C3%A9"), "café");
        // Already-decoded UTF-8 passes through untouched.
        assert_eq!(percent_decode("café"), "café");
        // Existing single-byte escapes still decode.
        assert_eq!(percent_decode("a%20b"), "a b");
    }

    #[test]
    fn write_clipped_lays_out_double_width_glyphs() {
        // Regression: one cell per char wrote the following char into a wide
        // glyph's spacer cell. A CJK glyph must occupy two cells so the next
        // char lands at column 2, not column 1.
        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        write_clipped(&mut buf, 0, 0, area.width, "世x", Style::default());

        assert_eq!(buf[(0, 0)].symbol(), "世");
        assert_eq!(buf[(2, 0)].symbol(), "x");
    }

    #[test]
    fn outer_terminal_pwd_sequence_forwards_osc7_file_uri() {
        assert_eq!(
            outer_terminal_pwd_sequence("file://localhost/tmp/glassterm%20project").as_deref(),
            Some("\x1b]7;file://localhost/tmp/glassterm%20project\x1b\\")
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
}
