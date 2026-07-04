//! Incremental scrollback search with native-selection highlighting.
//!
//! Search reads the terminal's full screen + scrollback text in one shot via a
//! plain-text [`Formatter`] dump (cheap, one FFI call — grid-ref traversal per
//! cell over 10k lines would be O(rows²)). Each dump line corresponds exactly
//! to one screen row `y` (blank rows are preserved; only trailing empty rows are
//! dropped), so a match's dump line index is its screen row.
//!
//! The current match is installed as the vt's native selection (the same
//! machinery the copy feature uses): two [`Point::Screen`] grid references
//! become a [`Selection`], which gives highlight rendering and scroll-tracking
//! for free. To place the endpoints on the correct columns even when a row
//! contains wide (double-width) glyphs, char offsets from the dump are mapped
//! to grid columns by walking the matched row's cells (skipping spacer tails).
//!
//! Case folding is ASCII-only, which keeps the folded char count identical to
//! the original so match offsets stay aligned.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use libghostty_vt::{
    Terminal,
    fmt::{Format, Formatter, FormatterOptions},
    screen::CellWide,
    selection::Selection,
    terminal::{Point, PointCoordinate, PointSpace, ScrollViewport},
};
use tracing::warn;

/// A single match within the screen, addressed by screen row and the character
/// offset/length within that row's dumped text (not grid columns — see the
/// module docs on wide-glyph column mapping).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Match {
    /// Screen-space row (history + active), 0-indexed from the top of history.
    pub(crate) row: u32,
    /// Character offset of the match within the row's dumped text.
    pub(crate) start_char: usize,
    /// Match length in characters.
    pub(crate) char_len: usize,
}

/// Scrollback search UI state.
pub(crate) struct SearchState {
    active: bool,
    query: String,
    matches: Vec<Match>,
    /// Index into `matches` of the current match (top-to-bottom order).
    current: usize,
    /// Set when terminal content changed (a vt_write batch or a resize) after
    /// the matches were last computed. `matches` addresses absolute screen rows,
    /// which output/scrollback-pruning/reflow silently invalidate; when stale we
    /// re-run the search before the next navigation rather than per output batch
    /// or per frame.
    stale: bool,
}

impl SearchState {
    pub(crate) fn new() -> Self {
        Self {
            active: false,
            query: String::new(),
            matches: Vec::new(),
            current: 0,
            stale: false,
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active
    }

    /// Toggle search mode on/off. Closing clears the match highlight but leaves
    /// the viewport where it is.
    pub(crate) fn toggle(&mut self, vt: &mut Terminal<'_, '_>) {
        if self.active {
            self.close(vt);
        } else {
            self.open();
        }
    }

    fn open(&mut self) {
        self.active = true;
        self.query.clear();
        self.matches.clear();
        self.current = 0;
        self.stale = false;
    }

    fn close(&mut self, vt: &mut Terminal<'_, '_>) {
        self.active = false;
        self.query.clear();
        self.matches.clear();
        self.current = 0;
        self.stale = false;
        let _ = vt.set_selection(None);
    }

    /// Note that terminal content changed (a vt_write batch or a resize) so the
    /// current match rows may be stale. Cheap: just flags the set for re-run on
    /// the next navigation. Call once per output batch, never per frame.
    pub(crate) fn note_content_changed(&mut self) {
        if self.active {
            self.stale = true;
        }
    }

    /// Recompute matches after a resize reflow and re-install the highlight
    /// immediately. Resize is rare and reflow invalidates every absolute row, so
    /// unlike streaming output we resolve it eagerly instead of on navigation.
    pub(crate) fn on_resize(&mut self, vt: &mut Terminal<'_, '_>) {
        if self.active {
            self.resync(vt);
            self.install(vt);
        }
    }

    /// Re-run the search for the current query against current content,
    /// clamping the current index into the fresh match set.
    fn resync(&mut self, vt: &Terminal<'_, '_>) {
        self.matches = find_matches(vt, &self.query);
        if self.current >= self.matches.len() {
            self.current = self.matches.len().saturating_sub(1);
        }
        self.stale = false;
    }

    /// Feed a key to the search UI. Returns whether a redraw is warranted.
    ///
    /// Callers must consume the key (not forward it to the child) whenever
    /// search is active; this handler ignores key releases.
    pub(crate) fn handle_key(&mut self, vt: &mut Terminal<'_, '_>, key: KeyEvent) -> bool {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                self.close(vt);
                true
            }
            KeyCode::Backspace => {
                self.backspace(vt);
                true
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.prev(vt);
                true
            }
            KeyCode::Enter => {
                self.next(vt);
                true
            }
            KeyCode::Char(c)
                if !key.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                self.push_char(vt, c);
                true
            }
            _ => false,
        }
    }

    pub(crate) fn push_char(&mut self, vt: &mut Terminal<'_, '_>, c: char) {
        self.query.push(c);
        self.recompute(vt);
    }

    pub(crate) fn backspace(&mut self, vt: &mut Terminal<'_, '_>) {
        self.query.pop();
        self.recompute(vt);
    }

    /// Move to the next match (upward, toward older output), wrapping.
    pub(crate) fn next(&mut self, vt: &mut Terminal<'_, '_>) {
        if self.stale {
            self.resync(vt);
        }
        if self.matches.is_empty() {
            return;
        }
        self.current = if self.current == 0 {
            self.matches.len() - 1
        } else {
            self.current - 1
        };
        self.install(vt);
    }

    /// Move to the previous match (downward, toward newer output), wrapping.
    pub(crate) fn prev(&mut self, vt: &mut Terminal<'_, '_>) {
        if self.stale {
            self.resync(vt);
        }
        if self.matches.is_empty() {
            return;
        }
        self.current = if self.current + 1 >= self.matches.len() {
            0
        } else {
            self.current + 1
        };
        self.install(vt);
    }

    /// Re-run the search from scratch and jump to the bottom-most match.
    fn recompute(&mut self, vt: &mut Terminal<'_, '_>) {
        self.matches = find_matches(vt, &self.query);
        // The most recent (bottom-most) match is last in top-to-bottom order.
        self.current = self.matches.len().saturating_sub(1);
        self.stale = false;
        self.install(vt);
    }

    /// Install the current match as the vt's active selection and scroll it
    /// into view. Clears the selection when there is no current match.
    fn install(&self, vt: &mut Terminal<'_, '_>) {
        let Some(m) = self.matches.get(self.current).copied() else {
            let _ = vt.set_selection(None);
            return;
        };
        let Some((start_col, end_col)) = resolve_columns(vt, m) else {
            warn!(row = m.row, "search could not resolve match columns");
            return;
        };

        {
            let start = match vt.grid_ref(screen(start_col, m.row)) {
                Ok(gr) => gr,
                Err(error) => {
                    warn!(%error, "search selection start grid_ref failed");
                    return;
                }
            };
            let end = match vt.grid_ref(screen(end_col, m.row)) {
                Ok(gr) => gr,
                Err(error) => {
                    warn!(%error, "search selection end grid_ref failed");
                    return;
                }
            };
            let selection = Selection::new(start, end, false);
            // set_selection borrows the vt immutably and pins the endpoints, so
            // it must happen before the mutating scroll below (which would
            // otherwise invalidate the untracked grid refs).
            let _ = vt.set_selection(Some(&selection));
        }

        scroll_into_view(vt, m.row);
    }

    /// The current match, if any.
    #[cfg(test)]
    pub(crate) fn current_match(&self) -> Option<Match> {
        self.matches.get(self.current).copied()
    }

    /// Number of matches for the current query.
    #[cfg(test)]
    pub(crate) fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// The status-bar prompt text for the current search state.
    pub(crate) fn prompt(&self) -> String {
        let suffix = if self.query.is_empty() {
            String::new()
        } else if self.matches.is_empty() {
            "  (no matches)".to_string()
        } else {
            format!("  ({}/{})", self.current + 1, self.matches.len())
        };
        // U+2588 FULL BLOCK stands in for a text cursor after the query.
        format!(" search: {}\u{2588}{suffix} ", self.query)
    }
}

/// Build a screen-space point.
fn screen(x: u16, y: u32) -> Point {
    Point::Screen(PointCoordinate { x, y })
}

/// ASCII-only case fold. Non-ASCII characters pass through unchanged so the
/// folded character count matches the original (keeping offsets aligned).
fn ascii_fold(c: char) -> char {
    c.to_ascii_lowercase()
}

/// Dump the full screen + scrollback as plain text, one line per screen row.
///
/// Uses `unwrap(false)` so each visual (grid) row is its own line, keeping the
/// line index equal to the screen row `y`. `trim(false)` preserves written
/// trailing spaces so offsets line up with the cell walk in [`resolve_columns`].
fn dump_lines(vt: &Terminal<'_, '_>) -> Vec<String> {
    let options = FormatterOptions::new()
        .with_format(Format::Plain)
        .with_unwrap(false)
        .with_trim(false);
    let mut formatter = match Formatter::new(vt, options) {
        Ok(formatter) => formatter,
        Err(error) => {
            warn!(%error, "search formatter creation failed");
            return Vec::new();
        }
    };
    let bytes = match formatter.format_alloc(None) {
        Ok(bytes) => bytes,
        Err(error) => {
            warn!(%error, "search screen dump failed");
            return Vec::new();
        }
    };
    String::from_utf8_lossy(&bytes)
        .split('\n')
        .map(str::to_string)
        .collect()
}

/// Find all (non-overlapping) matches of `query` across the screen and
/// scrollback, in top-to-bottom order. Case-insensitive (ASCII fold).
fn find_matches(vt: &Terminal<'_, '_>, query: &str) -> Vec<Match> {
    if query.is_empty() {
        return Vec::new();
    }
    let needle: Vec<char> = query.chars().map(ascii_fold).collect();
    let needle_len = needle.len();

    let mut matches = Vec::new();
    for (y, line) in dump_lines(vt).into_iter().enumerate() {
        let folded: Vec<char> = line.chars().map(ascii_fold).collect();
        if folded.len() < needle_len {
            continue;
        }
        let mut i = 0;
        while i + needle_len <= folded.len() {
            if folded[i..i + needle_len] == needle[..] {
                matches.push(Match {
                    row: y as u32,
                    start_char: i,
                    char_len: needle_len,
                });
                i += needle_len;
            } else {
                i += 1;
            }
        }
    }
    matches
}

/// Map a match's character offsets to inclusive grid columns by walking the
/// row's cells, mirroring exactly how the plain-text dump emits characters.
///
/// The formatter (see Ghostty `formatter.zig`) skips wide-glyph spacer cells
/// entirely and fills *interior* empty cells with a single space (only trailing
/// blanks are dropped). So every non-spacer cell contributes exactly one dump
/// character — a written codepoint or a filler space — and spacer cells
/// contribute none. Interior blanks (from `\x1b[NG`, CUP, or tabs) must
/// therefore be counted, or matches after such a gap fail to resolve. We stop at
/// the match's end offset, which is always a written cell, so the trailing-blank
/// exception never applies here.
///
/// Known limitation: cells holding multi-codepoint grapheme clusters emit more
/// than one character in the dump, which can shift columns for matches after
/// such a cell. Single-codepoint wide glyphs (CJK, most emoji) map correctly.
fn resolve_columns(vt: &Terminal<'_, '_>, m: Match) -> Option<(u16, u16)> {
    let cols = vt.cols().ok()?;
    let target_start = m.start_char;
    let target_end = m.start_char + m.char_len - 1;

    let mut seen = 0usize;
    let mut start_col = None;
    let mut end_col = None;
    for x in 0..cols {
        let grid_ref = vt.grid_ref(screen(x, m.row)).ok()?;
        // Wide-glyph spacer cells are omitted from the dump, so they contribute
        // no character and must not advance the offset counter.
        if matches!(
            grid_ref.cell().ok()?.wide().ok()?,
            CellWide::SpacerTail | CellWide::SpacerHead
        ) {
            continue;
        }
        if seen == target_start {
            start_col = Some(x);
        }
        if seen == target_end {
            end_col = Some(x);
            break;
        }
        seen += 1;
    }
    Some((start_col?, end_col?))
}

/// Scroll the viewport so the given screen row is visible. Leaves the viewport
/// untouched if the row is already visible; otherwise places it at the top of
/// the viewport.
fn scroll_into_view(vt: &mut Terminal<'_, '_>, row: u32) {
    if let Ok(grid_ref) = vt.grid_ref(screen(0, row))
        && matches!(
            vt.point_from_grid_ref(&grid_ref, PointSpace::Viewport),
            Ok(Some(_))
        )
    {
        return;
    }

    let Ok(top_ref) = vt.grid_ref(Point::Viewport(PointCoordinate { x: 0, y: 0 })) else {
        return;
    };
    let Ok(Some(top)) = vt.point_from_grid_ref(&top_ref, PointSpace::Screen) else {
        return;
    };
    let delta = row as isize - top.y as isize;
    vt.scroll_viewport(ScrollViewport::Delta(delta));
}

#[cfg(test)]
mod tests {
    use super::*;
    use libghostty_vt::{Terminal, TerminalOptions, selection::FormatOptions};

    /// A small terminal (rows=4) so most content lands in scrollback history.
    fn terminal_with(lines: &[&str]) -> Terminal<'static, 'static> {
        let mut vt = Terminal::new(TerminalOptions {
            cols: 40,
            rows: 4,
            max_scrollback: 1000,
        })
        .unwrap();
        for line in lines {
            vt.vt_write(line.as_bytes());
            vt.vt_write(b"\r\n");
        }
        vt
    }

    /// Read back the vt's currently installed active selection as plain text.
    fn active_selection_text(vt: &Terminal<'_, '_>) -> Option<String> {
        let options = FormatOptions::new()
            .with_emit_format(Format::Plain)
            .with_unwrap(true)
            .with_trim(true);
        vt.format_selection_alloc(None, options)
            .ok()
            .flatten()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }

    fn type_query(search: &mut SearchState, vt: &mut Terminal<'_, '_>, query: &str) {
        search.toggle(vt); // open
        for c in query.chars() {
            search.push_char(vt, c);
        }
    }

    #[test]
    fn finds_matches_across_scrollback_history() {
        // 10 lines, rows=4 -> at least 6 rows are in scrollback history.
        let lines: Vec<String> = (0..10).map(|i| format!("line{i:02} needle{i}")).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let vt = terminal_with(&refs);

        let matches = find_matches(&vt, "needle");
        assert_eq!(matches.len(), 10, "one match per line including history");
        // Rows are top-to-bottom and sequential.
        for (i, m) in matches.iter().enumerate() {
            assert_eq!(m.row, i as u32);
            assert_eq!(m.start_char, "line00 ".len());
            assert_eq!(m.char_len, "needle".len());
        }
    }

    #[test]
    fn jumps_to_bottom_most_match_and_installs_selection() {
        let refs = ["alpha match", "beta match", "gamma match"];
        let mut vt = terminal_with(&refs);
        let mut search = SearchState::new();

        type_query(&mut search, &mut vt, "match");

        assert_eq!(search.match_count(), 3);
        // Bottom-most match is on the last (gamma) line, screen row 2.
        let current = search.current_match().unwrap();
        assert_eq!(current.row, 2);
        assert_eq!(active_selection_text(&vt).as_deref(), Some("match"));
    }

    #[test]
    fn case_insensitive_matching() {
        let refs = ["Hello World", "goodbye world"];
        let mut vt = terminal_with(&refs);
        let mut search = SearchState::new();

        type_query(&mut search, &mut vt, "WORLD");

        assert_eq!(search.match_count(), 2);
        assert_eq!(active_selection_text(&vt).as_deref(), Some("world"));
    }

    #[test]
    fn re_search_on_query_edit() {
        let refs = ["need", "needle", "needless"];
        let mut vt = terminal_with(&refs);
        let mut search = SearchState::new();

        type_query(&mut search, &mut vt, "need");
        assert_eq!(search.match_count(), 3, "'need' matches all three lines");

        search.push_char(&mut vt, 'l');
        search.push_char(&mut vt, 'e');
        // "needle" matches "needle" and "needless".
        assert_eq!(search.match_count(), 2);

        // Editing back down re-broadens the match set.
        search.backspace(&mut vt);
        search.backspace(&mut vt);
        assert_eq!(search.match_count(), 3);
    }

    #[test]
    fn next_and_prev_wrap_and_move_selection() {
        let refs = ["one hit", "two hit", "three hit"];
        let mut vt = terminal_with(&refs);
        let mut search = SearchState::new();

        type_query(&mut search, &mut vt, "hit");
        // Start bottom-most (row 2).
        assert_eq!(search.current_match().unwrap().row, 2);

        // next() moves upward (toward older output).
        search.next(&mut vt);
        assert_eq!(search.current_match().unwrap().row, 1);
        search.next(&mut vt);
        assert_eq!(search.current_match().unwrap().row, 0);
        // Wraps back to the bottom.
        search.next(&mut vt);
        assert_eq!(search.current_match().unwrap().row, 2);

        // prev() moves downward and wraps too.
        search.prev(&mut vt);
        assert_eq!(search.current_match().unwrap().row, 0);

        assert_eq!(active_selection_text(&vt).as_deref(), Some("hit"));
    }

    #[test]
    fn current_match_scrolls_into_view() {
        let lines: Vec<String> = (0..20).map(|i| format!("row {i:02} target{i}")).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let mut vt = terminal_with(&refs);
        let mut search = SearchState::new();

        // Match an early line that is well inside scrollback history.
        type_query(&mut search, &mut vt, "target3");
        let m = search.current_match().unwrap();
        assert_eq!(search.match_count(), 1);

        // After install, the match row must be within the viewport.
        let grid_ref = vt.grid_ref(screen(0, m.row)).unwrap();
        let viewport = vt
            .point_from_grid_ref(&grid_ref, PointSpace::Viewport)
            .unwrap();
        assert!(
            viewport.is_some(),
            "match row should be scrolled into the viewport"
        );
    }

    #[test]
    fn selection_extracts_multiword_match() {
        let refs = ["the quick brown fox", "lazy dog"];
        let mut vt = terminal_with(&refs);
        let mut search = SearchState::new();

        type_query(&mut search, &mut vt, "quick brown");
        assert_eq!(search.match_count(), 1);
        assert_eq!(active_selection_text(&vt).as_deref(), Some("quick brown"));
    }

    #[test]
    fn navigation_resyncs_after_content_shift() {
        // Regression: matches address absolute rows; new output shifts/prunes
        // them. A content-change note must force a re-run before navigation so
        // the count and highlight reflect current content.
        let refs = ["one hit", "two hit"];
        let mut vt = terminal_with(&refs);
        let mut search = SearchState::new();

        type_query(&mut search, &mut vt, "hit");
        assert_eq!(search.match_count(), 2);

        // More output arrives (another match), shifting rows into history.
        vt.vt_write(b"three hit\r\n");
        search.note_content_changed();

        // Navigation must re-run the search first.
        search.next(&mut vt);
        assert_eq!(search.match_count(), 3, "navigation re-ran the stale search");
        assert_eq!(active_selection_text(&vt).as_deref(), Some("hit"));
    }

    #[test]
    fn resize_recomputes_matches_immediately() {
        let refs = ["needle here", "more needle"];
        let mut vt = terminal_with(&refs);
        let mut search = SearchState::new();

        type_query(&mut search, &mut vt, "needle");
        assert_eq!(search.match_count(), 2);

        // A resize reflow changes the layout; on_resize recomputes eagerly.
        vt.resize(20, 4, 0, 0).unwrap();
        search.on_resize(&mut vt);

        assert!(search.match_count() >= 1);
        assert_eq!(active_selection_text(&vt).as_deref(), Some("needle"));
    }

    #[test]
    fn resolves_match_columns_after_interior_cursor_gap() {
        // Regression: `\x1b[40G` leaves interior empty cells that the dump fills
        // with spaces; resolve_columns must count them or the match after the
        // gap never resolves ("search could not resolve match columns").
        let mut vt = Terminal::new(TerminalOptions {
            cols: 80,
            rows: 4,
            max_scrollback: 100,
        })
        .unwrap();
        vt.vt_write(b"left\x1b[40Gright");

        let mut search = SearchState::new();
        type_query(&mut search, &mut vt, "right");

        assert_eq!(search.match_count(), 1);
        assert_eq!(active_selection_text(&vt).as_deref(), Some("right"));
    }

    #[test]
    fn closing_clears_selection_and_state() {
        let refs = ["find me here"];
        let mut vt = terminal_with(&refs);
        let mut search = SearchState::new();

        type_query(&mut search, &mut vt, "find");
        assert!(active_selection_text(&vt).is_some());

        search.toggle(&mut vt); // close
        assert!(!search.is_active());
        assert_eq!(search.match_count(), 0);
        assert!(
            active_selection_text(&vt).is_none(),
            "closing search clears the highlight"
        );
    }

    #[test]
    fn empty_query_has_no_matches_or_selection() {
        let refs = ["something"];
        let mut vt = terminal_with(&refs);
        let mut search = SearchState::new();

        type_query(&mut search, &mut vt, "s");
        assert_eq!(search.match_count(), 1);
        // Delete back to empty.
        search.backspace(&mut vt);
        assert_eq!(search.match_count(), 0);
        assert!(active_selection_text(&vt).is_none());
    }
}
