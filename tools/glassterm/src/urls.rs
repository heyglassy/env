use std::{
    process::{Command, Stdio},
    thread,
};

use libghostty_vt::{
    Terminal,
    error::Error as VtError,
    terminal::{Point, PointCoordinate, PointSpace},
};
use tracing::warn;

use crate::render::CellPos;

const INITIAL_URI_BUF: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UrlMatch {
    pub(crate) url: String,
    pub(crate) spans: Vec<UrlSpan>,
}

impl UrlMatch {
    pub(crate) fn contains(&self, row: u16, col: u16) -> bool {
        self.spans
            .iter()
            .any(|span| span.row == row && col >= span.start_col && col <= span.end_col)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UrlSpan {
    pub(crate) row: u16,
    pub(crate) start_col: u16,
    pub(crate) end_col: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScreenCell {
    row: u32,
    col: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TextUrl {
    start: usize,
    end: usize,
    url: String,
}

/// Return the URL at a visible terminal cell, if any.
///
/// OSC 8 hyperlinks are resolved first via per-cell grid data. If the cell is
/// not an OSC 8 hyperlink, the soft-wrapped row group containing the cell is
/// scanned for textual URLs.
pub(crate) fn url_at_viewport_cell(vt: &Terminal<'_, '_>, cell: CellPos) -> Option<UrlMatch> {
    osc8_url_at_cell(vt, cell).or_else(|| text_url_at_cell(vt, cell))
}

/// Spawn macOS `open <url>` away from the event loop.
pub(crate) fn open_url(url: &str) {
    let url = url.to_string();
    let spawn = thread::Builder::new()
        .name("glassterm-open-url".to_string())
        .spawn(move || {
            match Command::new("open")
                .arg(&url)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(mut child) => {
                    if let Err(error) = child.wait() {
                        warn!(%error, url, "failed to wait for open command");
                    }
                }
                Err(error) => warn!(%error, url, "failed to spawn open command"),
            }
        });
    if let Err(error) = spawn {
        warn!(%error, "failed to spawn URL opener thread");
    }
}

fn osc8_url_at_cell(vt: &Terminal<'_, '_>, cell: CellPos) -> Option<UrlMatch> {
    let clicked = screen_cell_for_viewport(vt, cell)?;
    let target = hyperlink_uri_at(vt, clicked)?;
    let (start, end) = wrapped_row_group(vt, clicked.row)?;
    let cols = vt.cols().ok()?;

    let mut cells = Vec::new();
    let mut clicked_index = None;
    for row in start..=end {
        for col in 0..cols {
            let current = ScreenCell { row, col };
            if current == clicked {
                clicked_index = Some(cells.len());
            }
            cells.push((current, hyperlink_uri_at(vt, current)));
        }
    }

    let index = clicked_index?;
    let mut first = index;
    while first > 0 && cells[first - 1].1.as_deref() == Some(target.as_str()) {
        first -= 1;
    }
    let mut last = index;
    while last + 1 < cells.len() && cells[last + 1].1.as_deref() == Some(target.as_str()) {
        last += 1;
    }

    Some(UrlMatch {
        url: target,
        spans: visible_spans(vt, cells[first..=last].iter().map(|(cell, _)| *cell)),
    })
    .filter(|m| !m.spans.is_empty())
}

fn text_url_at_cell(vt: &Terminal<'_, '_>, cell: CellPos) -> Option<UrlMatch> {
    let clicked = screen_cell_for_viewport(vt, cell)?;
    let (start, end) = wrapped_row_group(vt, clicked.row)?;
    let (text, cells) = text_for_rows(vt, start, end)?;
    let clicked_char = cells.iter().position(|candidate| *candidate == clicked)?;

    let url = find_text_urls(&text)
        .into_iter()
        .find(|url| clicked_char >= url.start && clicked_char < url.end)?;
    Some(UrlMatch {
        spans: visible_spans(vt, cells[url.start..url.end].iter().copied()),
        url: url.url,
    })
    .filter(|m| !m.spans.is_empty())
}

fn screen_cell_for_viewport(vt: &Terminal<'_, '_>, cell: CellPos) -> Option<ScreenCell> {
    let grid_ref = vt
        .grid_ref(Point::Viewport(PointCoordinate {
            x: cell.col,
            y: u32::from(cell.row),
        }))
        .ok()?;
    let point = vt
        .point_from_grid_ref(&grid_ref, PointSpace::Screen)
        .ok()
        .flatten()?;
    Some(ScreenCell {
        row: point.y,
        col: point.x,
    })
}

fn viewport_cell_for_screen(vt: &Terminal<'_, '_>, cell: ScreenCell) -> Option<CellPos> {
    let grid_ref = vt
        .grid_ref(Point::Screen(PointCoordinate {
            x: cell.col,
            y: cell.row,
        }))
        .ok()?;
    let point = vt
        .point_from_grid_ref(&grid_ref, PointSpace::Viewport)
        .ok()
        .flatten()?;
    Some(CellPos {
        col: point.x,
        row: point.y as u16,
    })
}

fn wrapped_row_group(vt: &Terminal<'_, '_>, row: u32) -> Option<(u32, u32)> {
    let total_rows = vt.total_rows().ok()? as u32;
    if row >= total_rows {
        return None;
    }

    let mut start = row;
    while start > 0 && row_at(vt, start)?.is_wrap_continuation().ok()? {
        start -= 1;
    }

    let mut end = row;
    while end + 1 < total_rows && row_at(vt, end)?.is_wrapped().ok()? {
        end += 1;
    }

    Some((start, end))
}

fn row_at(vt: &Terminal<'_, '_>, row: u32) -> Option<libghostty_vt::screen::Row> {
    vt.grid_ref(Point::Screen(PointCoordinate { x: 0, y: row }))
        .ok()?
        .row()
        .ok()
}

fn text_for_rows(vt: &Terminal<'_, '_>, start: u32, end: u32) -> Option<(String, Vec<ScreenCell>)> {
    let cols = vt.cols().ok()?;
    let mut text = String::new();
    let mut cells = Vec::new();

    for row in start..=end {
        for col in 0..cols {
            let cell = ScreenCell { row, col };
            let grapheme = grapheme_at(vt, cell).unwrap_or_else(|| " ".to_string());
            let mut chars = grapheme.chars().peekable();
            if chars.peek().is_none() {
                text.push(' ');
                cells.push(cell);
            } else {
                for ch in chars {
                    text.push(ch);
                    cells.push(cell);
                }
            }
        }
    }

    Some((text, cells))
}

fn grapheme_at(vt: &Terminal<'_, '_>, cell: ScreenCell) -> Option<String> {
    let grid_ref = vt
        .grid_ref(Point::Screen(PointCoordinate {
            x: cell.col,
            y: cell.row,
        }))
        .ok()?;
    let mut buf = ['\0'; 8];
    let len = match grid_ref.graphemes(&mut buf) {
        Ok(len) => len,
        Err(VtError::OutOfSpace { required }) => {
            let mut buf = vec!['\0'; required];
            grid_ref.graphemes(&mut buf).ok()?;
            return Some(buf.into_iter().collect());
        }
        Err(_) => return None,
    };
    if len == 0 {
        None
    } else {
        Some(buf[..len].iter().collect())
    }
}

fn hyperlink_uri_at(vt: &Terminal<'_, '_>, cell: ScreenCell) -> Option<String> {
    let grid_ref = vt
        .grid_ref(Point::Screen(PointCoordinate {
            x: cell.col,
            y: cell.row,
        }))
        .ok()?;
    let mut buf = vec![0; INITIAL_URI_BUF];
    let len = match grid_ref.hyperlink_uri(&mut buf) {
        Ok(len) => len,
        Err(VtError::OutOfSpace { required }) => {
            buf.resize(required, 0);
            grid_ref.hyperlink_uri(&mut buf).ok()?
        }
        Err(_) => return None,
    };
    if len == 0 {
        None
    } else {
        String::from_utf8(buf[..len].to_vec()).ok()
    }
}

fn visible_spans(
    vt: &Terminal<'_, '_>,
    cells: impl IntoIterator<Item = ScreenCell>,
) -> Vec<UrlSpan> {
    let mut spans: Vec<UrlSpan> = Vec::new();
    for cell in cells {
        let Some(viewport) = viewport_cell_for_screen(vt, cell) else {
            continue;
        };
        if let Some(last) = spans.last_mut()
            && last.row == viewport.row
            && last.end_col.saturating_add(1) == viewport.col
        {
            last.end_col = viewport.col;
            continue;
        }
        spans.push(UrlSpan {
            row: viewport.row,
            start_col: viewport.col,
            end_col: viewport.col,
        });
    }
    spans
}

fn find_text_urls(text: &str) -> Vec<TextUrl> {
    let chars: Vec<char> = text.chars().collect();
    let mut urls = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let Some(scheme_len) = scheme_len_at(&chars, i) else {
            i += 1;
            continue;
        };

        let start = i;
        let mut end = start + scheme_len;
        while end < chars.len() && !chars[end].is_whitespace() {
            end += 1;
        }
        let stripped_end = strip_trailing_punctuation(&chars[start..end]) + start;
        if stripped_end > start + scheme_len {
            urls.push(TextUrl {
                start,
                end: stripped_end,
                url: chars[start..stripped_end].iter().collect(),
            });
        }
        i = end.max(start + 1);
    }
    urls
}

fn scheme_len_at(chars: &[char], start: usize) -> Option<usize> {
    for scheme in ["http://", "https://", "file://"] {
        let scheme_chars: Vec<char> = scheme.chars().collect();
        if chars
            .get(start..start + scheme_chars.len())
            .is_some_and(|candidate| candidate == scheme_chars.as_slice())
        {
            return Some(scheme_chars.len());
        }
    }
    None
}

fn strip_trailing_punctuation(chars: &[char]) -> usize {
    let mut end = chars.len();
    while end > 0 {
        let ch = chars[end - 1];
        if ch == ')' {
            let opens = chars[..end].iter().filter(|c| **c == '(').count();
            let closes = chars[..end].iter().filter(|c| **c == ')').count();
            if closes <= opens {
                break;
            }
            end -= 1;
        } else if matches!(
            ch,
            ']' | '}' | '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"'
        ) {
            end -= 1;
        } else {
            break;
        }
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;
    use libghostty_vt::{Terminal, TerminalOptions};

    fn urls(text: &str) -> Vec<String> {
        find_text_urls(text)
            .into_iter()
            .map(|url| url.url)
            .collect()
    }

    fn terminal_with(content: &[u8]) -> Terminal<'static, 'static> {
        let mut vt = Terminal::new(TerminalOptions {
            cols: 80,
            rows: 5,
            max_scrollback: 100,
        })
        .unwrap();
        vt.vt_write(content);
        vt
    }

    #[test]
    fn scanner_finds_http_https_and_file_urls() {
        assert_eq!(
            urls("see http://example.test https://example.test file:///tmp/a"),
            vec![
                "http://example.test",
                "https://example.test",
                "file:///tmp/a"
            ]
        );
    }

    #[test]
    fn scanner_strips_trailing_punctuation() {
        assert_eq!(
            urls("open https://example.test/path). and http://a.test/x?!"),
            vec!["https://example.test/path", "http://a.test/x"]
        );
    }

    #[test]
    fn scanner_keeps_balanced_trailing_paren() {
        assert_eq!(
            urls("wiki https://example.test/Foo_(bar))"),
            vec!["https://example.test/Foo_(bar)"]
        );
    }

    #[test]
    fn scanner_returns_no_match_for_plain_text_rows() {
        assert!(find_text_urls("nothing to open here").is_empty());
    }

    #[test]
    fn scanner_finds_urls_at_row_start_and_end() {
        assert_eq!(
            urls("https://start.test middle file:///tmp/end"),
            vec!["https://start.test", "file:///tmp/end"]
        );
    }

    #[test]
    fn scanner_finds_adjacent_urls_when_whitespace_separated() {
        assert_eq!(
            urls("http://one.test https://two.test"),
            vec!["http://one.test", "https://two.test"]
        );
    }

    #[test]
    fn coordinate_lookup_extracts_text_url_from_vt_row() {
        let vt = terminal_with(b"prefix https://example.test/path suffix");
        let found = url_at_viewport_cell(&vt, CellPos { col: 12, row: 0 }).unwrap();

        assert_eq!(found.url, "https://example.test/path");
        assert!(found.contains(0, 7));
        assert!(found.contains(0, 31));
        assert!(!found.contains(0, 6));
    }

    #[test]
    fn osc8_hyperlink_target_wins_over_text_scan() {
        let vt = terminal_with(
            b"\x1b]8;;https://target.example\x1b\\https://label.example\x1b]8;;\x1b\\",
        );
        let found = url_at_viewport_cell(&vt, CellPos { col: 0, row: 0 }).unwrap();

        assert_eq!(found.url, "https://target.example");
        assert!(found.contains(0, 0));
    }
}
