//! Mouse-driven text selection backed by libghostty-vt's native selection.
//!
//! Rather than tracking cell rectangles in glassterm (the old overlay
//! approach), we drive libghostty's selection gesture state machine. Pointer
//! presses/drags/releases are translated into gesture events; the resulting
//! [`Selection`](libghostty_vt::selection::Selection) snapshot is installed as
//! the terminal's active selection. Because the selection lives inside the vt
//! (as tracked pins), it scrolls with scrollback content for free, and the
//! renderer paints it from the render snapshot's per-row selection range.
//!
//! The gesture API also gives us word (double-click) and line (triple-click)
//! selection through its built-in click-count behavior table.

use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};
use libghostty_vt::{
    Terminal,
    fmt::Format,
    selection::{
        FormatOptions,
        gesture::{Behavior, DragEvent, Geometry, Gesture, PressEvent, ReleaseEvent},
    },
    terminal::{Point, PointCoordinate},
};
use ratatui::layout::Rect;
use tracing::warn;

use crate::render::mouse_to_cell;

/// Maximum interval between clicks to count as a repeat (double/triple) click.
const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);
/// Synthetic per-cell width in surface pixels.
///
/// Our pointer input is cell-granular, but the gesture API is designed for
/// pixel input and applies half-cell rounding to decide selection boundaries.
/// We map each cell to a block of pixels so we have enough sub-cell resolution
/// to place the caret precisely within a cell (a `cell_width` of 1 does not
/// give enough resolution and drops the hovered cell).
const CELL_PX: f64 = 16.0;
/// Maximum pointer travel (in pixels) between clicks to count as a repeat click.
const REPEAT_DISTANCE_PX: f64 = CELL_PX;

/// The result of feeding a mouse event to the selection state machine.
#[derive(Debug, Default)]
pub(crate) struct SelectionOutcome {
    /// Whether the visible selection changed and a redraw is warranted.
    pub(crate) changed: bool,
    /// Extracted selection text to copy to the clipboard (copy-on-select).
    pub(crate) copy: Option<String>,
}

/// Owns the reusable gesture state and typed gesture events.
pub(crate) struct MouseSelection {
    gesture: Gesture<'static>,
    press: PressEvent<'static>,
    drag: DragEvent<'static>,
    release: ReleaseEvent<'static>,
    base: Instant,
    active: bool,
    /// The cell of the active left-click anchor, if any. Needed to re-issue the
    /// press with a direction-aware caret when a cell drag reverses direction.
    press_cell: Option<(u16, u16)>,
    /// Whether the currently-baked press caret sits on the right of its cell.
    /// The gesture bakes the anchor's x-position at press time and offers no way
    /// to move it afterwards, so we track the bias and re-press when it must flip.
    press_caret_right: bool,
}

impl MouseSelection {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            gesture: Gesture::new()?,
            press: PressEvent::new()?,
            drag: DragEvent::new()?,
            release: ReleaseEvent::new()?,
            base: Instant::now(),
            active: false,
            press_cell: None,
            press_caret_right: false,
        })
    }

    /// Feed a left-button mouse event to the selection state machine.
    ///
    /// Non-left events return an empty outcome. All libghostty errors are
    /// logged and swallowed so the terminal keeps running.
    pub(crate) fn handle_mouse(
        &mut self,
        vt: &Terminal<'_, '_>,
        mouse: MouseEvent,
        area: Rect,
    ) -> SelectionOutcome {
        let mut outcome = SelectionOutcome::default();
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(cell) = mouse_to_cell(mouse, area) else {
                    outcome.changed = self.clear(vt);
                    return outcome;
                };
                match self.apply_press(vt, cell.col, cell.row) {
                    Ok(changed) => outcome.changed = changed,
                    Err(error) => warn!(%error, "selection press failed"),
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some(cell) = mouse_to_cell(mouse, area) else {
                    return outcome;
                };
                match self.apply_drag(vt, cell.col, cell.row, area) {
                    Ok(changed) => outcome.changed = changed,
                    Err(error) => warn!(%error, "selection drag failed"),
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let cell = mouse_to_cell(mouse, area).map(|c| (c.col, c.row));
                match self.apply_release(vt, cell) {
                    Ok(copy) => outcome.copy = copy,
                    Err(error) => warn!(%error, "selection release failed"),
                }
            }
            _ => {}
        }
        outcome
    }

    fn apply_press(&mut self, vt: &Terminal<'_, '_>, x: u16, y: u16) -> Result<bool> {
        // Anchor the press caret on the *left* of the cell by default. A
        // forward (left-to-right / downward) drag then includes the clicked
        // cell for free, so no re-press is needed in the common case.
        let is_some = self.issue_press(vt, x, y, false)?;
        self.press_cell = Some((x, y));
        self.press_caret_right = false;
        self.active = is_some;
        Ok(true)
    }

    /// Issue a press event to the gesture with the caret biased to the left or
    /// right of the pressed cell, installing the resulting selection. Returns
    /// whether a selection was produced (word/line press) or not (cell press).
    fn issue_press(
        &mut self,
        vt: &Terminal<'_, '_>,
        x: u16,
        y: u16,
        caret_right: bool,
    ) -> Result<bool> {
        let grid_ref = vt.grid_ref(Point::Viewport(PointCoordinate { x, y: y as u32 }))?;
        self.press
            .set_position(caret_px(x, caret_right), cell_center_px(y))?
            .set_time(self.base.elapsed())?
            .set_repeat_interval(DOUBLE_CLICK_INTERVAL)?
            .set_repeat_distance(REPEAT_DISTANCE_PX)?;
        let selection = self.press.apply(&mut self.gesture, vt, grid_ref)?;
        vt.set_selection(selection.as_ref())?;
        Ok(selection.is_some())
    }

    fn apply_drag(&mut self, vt: &Terminal<'_, '_>, x: u16, y: u16, area: Rect) -> Result<bool> {
        // Ghostty's cell drag selection only includes a cell when the caret is
        // on the correct side of the cell's 60% threshold for the drag
        // direction (see SelectionGesture.zig `dragSelection`). To keep BOTH
        // the pressed cell and the dragged-to cell selected regardless of
        // direction, bias the drag caret to the right of its cell for forward
        // drags and to the left for backward drags, and re-issue the press with
        // the mirrored bias when the direction flips.
        //
        // "Forward" is reading order: the drag cell is at or after the pressed
        // cell (a later row, or the same row and an equal/greater column).
        let forward = match self.press_cell {
            Some((px, py)) => (y, x) >= (py, px),
            None => true,
        };

        // The pressed cell is included when the press caret is on the left for
        // forward drags and on the right for backward drags. The caret is baked
        // at press time, so if the required bias differs from what is baked and
        // this is a cell (single-click) gesture, re-press to move it. Word/line
        // gestures ignore the caret fraction entirely, so leave them untouched.
        let need_right = !forward;
        if need_right != self.press_caret_right {
            if let Some((px, py)) = self.press_cell {
                if matches!(self.gesture.behavior(vt), Ok(Behavior::Cell)) {
                    self.gesture.reset(vt);
                    self.issue_press(vt, px, py, need_right)?;
                    self.press_caret_right = need_right;
                }
            }
        }

        let grid_ref = vt.grid_ref(Point::Viewport(PointCoordinate { x, y: y as u32 }))?;
        self.drag
            .set_position(caret_px(x, forward), cell_center_px(y))?;
        let selection = self
            .drag
            .apply(&mut self.gesture, vt, grid_ref, geometry_for(area))?;
        if let Some(sel) = &selection {
            vt.set_selection(Some(sel))?;
            self.active = true;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn apply_release(
        &mut self,
        vt: &Terminal<'_, '_>,
        cell: Option<(u16, u16)>,
    ) -> Result<Option<String>> {
        let grid_ref = match cell {
            Some((x, y)) => Some(vt.grid_ref(Point::Viewport(PointCoordinate { x, y: y as u32 }))?),
            None => None,
        };
        self.release.apply(&mut self.gesture, vt, grid_ref)?;
        if !self.active {
            return Ok(None);
        }
        extract_selection(vt)
    }

    /// Clear the active selection (e.g. on key press). Returns whether there
    /// was a selection to clear.
    pub(crate) fn clear(&mut self, vt: &Terminal<'_, '_>) -> bool {
        let was_active = self.active;
        if was_active {
            let _ = vt.set_selection(None);
        }
        self.gesture.reset(vt);
        self.active = false;
        self.press_cell = None;
        self.press_caret_right = false;
        was_active
    }
}

/// Extract the terminal's active selection as clipboard-ready plain text.
///
/// Matches Ghostty's copy behavior: plain text, soft-wrapped lines unwrapped,
/// trailing whitespace trimmed. Whitespace-only extractions yield `None`.
fn extract_selection(vt: &Terminal<'_, '_>) -> Result<Option<String>> {
    let options = FormatOptions::new()
        .with_emit_format(Format::Plain)
        .with_unwrap(true)
        .with_trim(true);
    let Some(bytes) = vt.format_selection_alloc(None, options)? else {
        return Ok(None);
    };
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if text.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(text))
    }
}

/// The surface-pixel center of a given cell index along one axis.
fn cell_center_px(index: u16) -> f64 {
    f64::from(index) * CELL_PX + CELL_PX / 2.0
}

/// The surface-pixel x of a caret biased to include the given cell for a drag.
///
/// Ghostty includes a cell in a cell drag only when the caret is on the correct
/// side of the cell's ~60% threshold for the drag direction. `caret_right`
/// places the caret at the right edge (fraction above the threshold) so the
/// cell is included on a forward drag; otherwise it sits at the left edge
/// (fraction below the threshold) for a backward drag.
fn caret_px(index: u16, caret_right: bool) -> f64 {
    let base = f64::from(index) * CELL_PX;
    if caret_right {
        base + (CELL_PX - 1.0)
    } else {
        base
    }
}

/// Build the display geometry for gesture drags. Each cell is [`CELL_PX`]
/// surface pixels wide/tall. All fields must be non-zero.
fn geometry_for(area: Rect) -> Geometry {
    Geometry {
        columns: u32::from(area.width).max(1),
        cell_width: CELL_PX as u32,
        padding_left: 0,
        screen_height: (u32::from(area.height).max(1)) * CELL_PX as u32,
    }
}

/// Whether a key event should clear the current selection (any press/repeat).
pub(crate) fn key_clears_selection(key: KeyEvent) -> bool {
    matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
}

/// Whether a mouse event is a left-button press/drag/release (the events that
/// drive glassterm-side selection).
pub(crate) fn is_left_button_event(kind: MouseEventKind) -> bool {
    matches!(
        kind,
        MouseEventKind::Down(MouseButton::Left)
            | MouseEventKind::Drag(MouseButton::Left)
            | MouseEventKind::Up(MouseButton::Left)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};
    use libghostty_vt::{Terminal, TerminalOptions};

    fn terminal_with(content: &[u8]) -> Terminal<'static, 'static> {
        let mut vt = Terminal::new(TerminalOptions {
            cols: 40,
            rows: 5,
            max_scrollback: 100,
        })
        .unwrap();
        vt.vt_write(content);
        vt
    }

    fn mouse(kind: MouseEventKind, col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column: col,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn drag_selection_extracts_plain_text() {
        let area = Rect::new(0, 0, 40, 5);
        let vt = terminal_with(b"hello world");
        let mut sel = MouseSelection::new().unwrap();

        sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Down(MouseButton::Left), 0, 0),
            area,
        );
        sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Drag(MouseButton::Left), 4, 0),
            area,
        );
        let outcome = sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Up(MouseButton::Left), 4, 0),
            area,
        );

        assert_eq!(outcome.copy.as_deref(), Some("hello"));
    }

    #[test]
    fn rtl_drag_extracts_full_word() {
        // Regression: a right-to-left drag used to drop both endpoint cells
        // (pressing col 4 of "hello" and dragging to col 0 copied "ell").
        let area = Rect::new(0, 0, 40, 5);
        let vt = terminal_with(b"hello");
        let mut sel = MouseSelection::new().unwrap();

        sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Down(MouseButton::Left), 4, 0),
            area,
        );
        sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Drag(MouseButton::Left), 0, 0),
            area,
        );
        let outcome = sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Up(MouseButton::Left), 0, 0),
            area,
        );

        assert_eq!(outcome.copy.as_deref(), Some("hello"));
    }

    #[test]
    fn multi_row_upward_drag_extracts_text() {
        // Regression: an upward (backward) drag across rows must include both
        // the pressed cell and the dragged-to cell.
        let area = Rect::new(0, 0, 40, 5);
        let vt = terminal_with(b"abc\r\ndef");
        let mut sel = MouseSelection::new().unwrap();

        // Press the last cell of row 1, drag up to the first cell of row 0.
        sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Down(MouseButton::Left), 2, 1),
            area,
        );
        sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Drag(MouseButton::Left), 0, 0),
            area,
        );
        let outcome = sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Up(MouseButton::Left), 0, 0),
            area,
        );

        assert_eq!(outcome.copy.as_deref(), Some("abc\ndef"));
    }

    #[test]
    fn single_cell_click_drag_in_place_selects_one_cell() {
        // A press and a drag that stays on the same cell selects that one cell.
        let area = Rect::new(0, 0, 40, 5);
        let vt = terminal_with(b"hello");
        let mut sel = MouseSelection::new().unwrap();

        sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Down(MouseButton::Left), 2, 0),
            area,
        );
        sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Drag(MouseButton::Left), 2, 0),
            area,
        );
        let outcome = sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Up(MouseButton::Left), 2, 0),
            area,
        );

        assert_eq!(outcome.copy.as_deref(), Some("l"));
    }

    #[test]
    fn double_click_selects_word() {
        let area = Rect::new(0, 0, 40, 5);
        let vt = terminal_with(b"hello world");
        let mut sel = MouseSelection::new().unwrap();

        // Two rapid clicks on a cell inside "world" (col 8) => word selection.
        sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Down(MouseButton::Left), 8, 0),
            area,
        );
        sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Up(MouseButton::Left), 8, 0),
            area,
        );
        sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Down(MouseButton::Left), 8, 0),
            area,
        );
        let outcome = sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Up(MouseButton::Left), 8, 0),
            area,
        );

        assert_eq!(outcome.copy.as_deref(), Some("world"));
    }

    #[test]
    fn drag_selection_trims_trailing_whitespace() {
        let area = Rect::new(0, 0, 40, 5);
        let vt = terminal_with(b"hi");
        let mut sel = MouseSelection::new().unwrap();

        sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Down(MouseButton::Left), 0, 0),
            area,
        );
        // Drag well past the text into blank cells.
        sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Drag(MouseButton::Left), 20, 0),
            area,
        );
        let outcome = sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Up(MouseButton::Left), 20, 0),
            area,
        );

        assert_eq!(outcome.copy.as_deref(), Some("hi"));
    }

    #[test]
    fn single_click_without_drag_copies_nothing() {
        let area = Rect::new(0, 0, 40, 5);
        let vt = terminal_with(b"hello world");
        let mut sel = MouseSelection::new().unwrap();

        sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Down(MouseButton::Left), 2, 0),
            area,
        );
        let outcome = sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Up(MouseButton::Left), 2, 0),
            area,
        );

        assert_eq!(outcome.copy, None);
    }

    #[test]
    fn clear_reports_whether_selection_was_active() {
        let area = Rect::new(0, 0, 40, 5);
        let vt = terminal_with(b"hello world");
        let mut sel = MouseSelection::new().unwrap();

        assert!(!sel.clear(&vt));

        sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Down(MouseButton::Left), 0, 0),
            area,
        );
        sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Drag(MouseButton::Left), 4, 0),
            area,
        );
        assert!(sel.clear(&vt));
        assert!(!sel.clear(&vt));
    }

    #[test]
    fn search_open_close_leaves_no_phantom_active_selection() {
        // Regression: a live drag-selection used to keep MouseSelection.active
        // true across a whole search session; after Esc-closing search, the
        // next Esc was swallowed clearing that phantom instead of reaching the
        // child. Opening search must clear the selection so no phantom remains.
        use crate::search::SearchState;

        let area = Rect::new(0, 0, 40, 5);
        let mut vt = terminal_with(b"hello world");
        let mut sel = MouseSelection::new().unwrap();
        let mut search = SearchState::new();

        // Drag-select so the mouse selection is active.
        sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Down(MouseButton::Left), 0, 0),
            area,
        );
        sel.handle_mouse(
            &vt,
            mouse(MouseEventKind::Drag(MouseButton::Left), 4, 0),
            area,
        );

        // Opening search clears the mouse selection (main-loop wiring): there
        // was an active selection to clear.
        assert!(sel.clear(&vt));
        search.toggle(&mut vt); // open
        search.toggle(&mut vt); // Esc-close

        // No phantom active selection remains to swallow a later Esc.
        assert!(!sel.clear(&vt));
    }

    #[test]
    fn key_clears_selection_on_press_and_repeat_only() {
        assert!(key_clears_selection(KeyEvent::new_with_kind(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )));
        assert!(key_clears_selection(KeyEvent::new_with_kind(
            KeyCode::Down,
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )));
        assert!(!key_clears_selection(KeyEvent::new_with_kind(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        )));
    }

    #[test]
    fn left_button_events_are_recognized() {
        assert!(is_left_button_event(MouseEventKind::Down(
            MouseButton::Left
        )));
        assert!(is_left_button_event(MouseEventKind::Drag(
            MouseButton::Left
        )));
        assert!(is_left_button_event(MouseEventKind::Up(MouseButton::Left)));
        assert!(!is_left_button_event(MouseEventKind::Moved));
        assert!(!is_left_button_event(MouseEventKind::Down(
            MouseButton::Right
        )));
    }
}
