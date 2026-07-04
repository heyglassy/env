use crossterm::event::MouseEvent;
use libghostty_vt::{
    Terminal,
    render::{CellIterator, CursorVisualStyle, RenderState, RowIterator},
    style::RgbColor,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::urls::UrlMatch;

pub(crate) struct GhosttyRenderer;

impl GhosttyRenderer {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render(
        &mut self,
        vt: &mut Terminal<'static, 'static>,
        render_state: &mut RenderState<'static>,
        area: Rect,
        buf: &mut Buffer,
        cursor_text: RgbColor,
        selection_fg: RgbColor,
        selection_bg: RgbColor,
        hover_url: Option<&UrlMatch>,
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

            let selection = row.selection().ok().flatten();
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
                let selected = selection
                    .as_ref()
                    .is_some_and(|range| x >= range.start_x && x <= range.end_x);
                let mut style = if selected {
                    Style::default().fg(rgb(selection_fg)).bg(rgb(selection_bg))
                } else {
                    let fg = cell
                        .fg_color()
                        .ok()
                        .flatten()
                        .map(rgb)
                        .unwrap_or(default_fg);
                    let bg = cell.bg_color().ok().flatten().map(rgb);
                    terminal_cell_style(fg, bg)
                };
                if hover_url.is_some_and(|url| url.contains(y, x)) {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                let cell = &mut buf[(area.x + x, area.y + y)];
                cell.set_symbol(&symbol);
                cell.set_style(style);
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

// `bg` is `Some` only when the cell carries an explicit background
// (SGR/palette/rgb); an unstyled cell passes `None` here. Mapping `None` to
// `Color::Reset` (rather than the theme background) is what lets the host
// terminal's `background-image` show through — see the passthrough-contract
// comment on `Theme::apply_to_vt`. Do not substitute the theme background here.
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

pub(crate) fn rgb(color: RgbColor) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellPos {
    pub(crate) col: u16,
    pub(crate) row: u16,
}

pub(crate) fn constrain_cell(pos: CellPos, area: Rect) -> Option<CellPos> {
    if pos.col < area.width && pos.row < area.height {
        Some(pos)
    } else {
        None
    }
}

pub(crate) fn mouse_to_cell(mouse: MouseEvent, area: Rect) -> Option<CellPos> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyModifiers, MouseEventKind};

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
    fn default_terminal_background_resets_to_host_surface() {
        let style = terminal_cell_style(Color::White, None);

        assert_eq!(style.bg, Some(Color::Reset));
    }

    #[test]
    fn explicit_terminal_background_is_preserved() {
        let style = terminal_cell_style(Color::White, Some(Color::Blue));

        assert_eq!(style.bg, Some(Color::Blue));
    }

    // Pinning test for the background-image passthrough contract.
    //
    // glassterm relies on Ghostty (the host terminal) to draw the configured
    // `background-image`; glassterm makes it show through by painting unstyled
    // cells with `Color::Reset` (the host surface). That only works if an
    // unstyled cell's `bg_color()` comes back `None` EVEN after we call
    // `set_default_bg_color(Some(theme.background))` in `Theme::apply_to_vt`.
    // libghostty-vt documents that the cell-level `bg_color()` reports only an
    // *explicit* background (SGR / palette / rgb) and returns `None` otherwise,
    // leaving the terminal default to the caller. This test proves that empiric-
    // ally so a future libghostty-vt bump can't silently start baking the default
    // background into every cell (which would paint opaque cells and hide the
    // image).
    #[test]
    fn unstyled_cell_reports_no_bg_despite_default_bg_color() {
        use libghostty_vt::{Terminal, TerminalOptions};

        let mut vt = Terminal::new(TerminalOptions {
            cols: 20,
            rows: 3,
            max_scrollback: 10,
        })
        .unwrap();
        // Mimic Theme::apply_to_vt: install an opaque default background.
        vt.set_default_bg_color(Some(RgbColor {
            r: 0x1a,
            g: 0x1b,
            b: 0x26,
        }))
        .unwrap();
        // Row 0: plain text, no styling. Row 1: text with an explicit SGR
        // background (blue, palette 44).
        vt.vt_write(b"plain\r\n\x1b[44mBG\x1b[0m");

        let mut render_state = RenderState::new().unwrap();
        let snapshot = render_state.update(&mut vt).unwrap();
        let mut row_iter = RowIterator::new().unwrap();
        let mut cell_iter = CellIterator::new().unwrap();
        let mut rows = row_iter.update(&snapshot).unwrap();

        // Row 0, first cell = 'p' (unstyled): must report NO background so the
        // renderer falls back to Color::Reset and the host image shows through.
        let row0 = rows.next().unwrap();
        let mut cells0 = cell_iter.update(row0).unwrap();
        let plain = cells0.next().unwrap();
        assert_eq!(
            plain.bg_color().unwrap(),
            None,
            "unstyled cell must report no bg even after set_default_bg_color"
        );
        drop(cells0);

        // Row 1, first cell = 'B' (explicit SGR bg): must report a background so
        // child-chosen colors are still painted opaquely.
        let row1 = rows.next().unwrap();
        let mut cells1 = cell_iter.update(row1).unwrap();
        let styled = cells1.next().unwrap();
        assert!(
            styled.bg_color().unwrap().is_some(),
            "cell with an explicit SGR background must report that background"
        );
    }
}
