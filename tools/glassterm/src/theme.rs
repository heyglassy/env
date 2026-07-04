use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use libghostty_vt::{
    Terminal,
    style::{PaletteIndex, RgbColor},
};
use tracing::{debug, info, warn};

use crate::{
    config::{AppearancePreference, Config, key_value_lines},
    overlay::InteractionStyle,
    status::StatusStyle,
};

const GHOSTTY_CONFIG_TIMEOUT: Duration = Duration::from_millis(250);
const MACOS_APPEARANCE_TIMEOUT: Duration = Duration::from_millis(100);
const COLOR_QUERY_TIMEOUT: Duration = Duration::from_millis(120);
const COLOR_QUERY_QUIET: Duration = Duration::from_millis(10);
const ANSI_PALETTE_QUERY_COUNT: usize = 16;

#[derive(Clone, Debug)]
pub(crate) struct Theme {
    pub(crate) foreground: RgbColor,
    pub(crate) background: RgbColor,
    pub(crate) cursor_color: RgbColor,
    pub(crate) cursor_text: RgbColor,
    pub(crate) selection_foreground: RgbColor,
    pub(crate) selection_background: RgbColor,
    pub(crate) palette: [RgbColor; 256],
    pub(crate) background_image: Option<String>,
    pub(crate) background_image_opacity: Option<f32>,
    pub(crate) theme_setting: Option<String>,
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
    pub(crate) fn query(config: &Config) -> Result<Self> {
        if !config.theme_query {
            info!("theme query disabled by config");
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

        if !config.osc_theme_query {
            info!("OSC theme query disabled by config");
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

        let (appearance, appearance_source) = match config.appearance {
            AppearancePreference::Light => (Appearance::Light, "glassterm config"),
            AppearancePreference::Dark => (Appearance::Dark, "glassterm config"),
            AppearancePreference::Auto => match Appearance::from_macos() {
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
            },
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

    // Background-image passthrough contract:
    // `set_default_bg_color` installs the terminal DEFAULT background used to
    // resolve foreground contrast, but libghostty-vt keeps it separate from
    // per-cell backgrounds: an unstyled cell's `bg_color()` still reports `None`
    // (only explicit SGR/palette/rgb backgrounds are reported). render.rs maps
    // that `None` to `Color::Reset`, so unstyled cells stay transparent and any
    // Ghostty `background-image` shows through. See the pinning test
    // `render::tests::unstyled_cell_reports_no_bg_despite_default_bg_color`.
    pub(crate) fn apply_to_vt(&self, vt: &mut Terminal<'_, '_>) -> Result<()> {
        vt.set_default_fg_color(Some(self.foreground))?
            .set_default_bg_color(Some(self.background))?
            .set_default_cursor_color(Some(self.cursor_color))?
            .set_default_color_palette(Some(self.palette))?;
        Ok(())
    }

    pub(crate) fn status_style(&self) -> StatusStyle {
        StatusStyle {
            foreground: self.selection_foreground,
            background: self.selection_background,
        }
    }

    pub(crate) fn interaction_style(&self) -> InteractionStyle {
        InteractionStyle {
            foreground: self.selection_foreground,
            background: self.selection_background,
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
    for (key, value) in key_value_lines(config) {
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

pub(crate) fn format_rgb(color: RgbColor) -> String {
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

pub(crate) fn parse_hex_rgb(input: &str) -> Option<RgbColor> {
    let hex = input.trim().strip_prefix('#')?;
    // Guard against non-ASCII input: `len()`/byte-slicing below assume one byte
    // per char, so a multi-byte char (e.g. "#€abc") would slice mid-codepoint
    // and panic. Requiring all hex digits keeps the byte indexing valid.
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
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
    fn parse_hex_rgb_rejects_non_ascii_without_panicking() {
        // Regression: byte-length check + byte slicing panicked on multi-byte
        // input like "#€abc" (would abort at startup on a bad config line).
        assert_eq!(parse_hex_rgb("#\u{20ac}abc"), None);
        // A valid six-digit hex color still parses.
        assert_eq!(
            parse_hex_rgb("#1a2b3c"),
            Some(RgbColor {
                r: 0x1a,
                g: 0x2b,
                b: 0x3c
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
background-image = /Users/heyglassy/.config/ghostty/backgrounds/glassterm.png
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
            Some("/Users/heyglassy/.config/ghostty/backgrounds/glassterm.png")
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
background-image = /Users/heyglassy/.config/ghostty/backgrounds/glassterm.png
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
            Some("/Users/heyglassy/.config/ghostty/backgrounds/glassterm.png")
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
    }
}
