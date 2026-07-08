use anyhow::Result;
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MediaKeyCode, ModifierKeyCode, MouseButton,
    MouseEvent, MouseEventKind,
};
use libghostty_vt::{
    Terminal, focus as ghostty_focus, key as ghostty_key, mouse as ghostty_mouse, paste,
};
use ratatui::layout::Rect;

/// Tracks whether a Super/Cmd (or Meta) modifier key is currently held down,
/// derived from Kitty keyboard-protocol press/release events.
///
/// SGR mouse reporting (mode 1006) encodes only Shift, Alt/Meta and Ctrl — the
/// protocol has no Super/Cmd bit — so crossterm can never set
/// `KeyModifiers::SUPER` on a `MouseEvent`. To recognize a Cmd+left-click we
/// instead remember the Super key's held state from keyboard events, which the
/// Kitty keyboard protocol *does* report (including releases, since glassterm
/// pushes REPORT_EVENT_TYPES + REPORT_ALL_KEYS_AS_ESCAPE_CODES).
#[derive(Debug, Default)]
pub(crate) struct SuperKeyState {
    held: bool,
}

impl SuperKeyState {
    /// Whether a Super/Cmd/Meta key is currently held.
    pub(crate) fn is_held(&self) -> bool {
        self.held
    }

    /// Update held state from a keyboard event. Returns true when the event was
    /// a Super/Meta modifier key press/release (informational; the event is
    /// still forwarded to the child by the caller — unchanged behavior).
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> bool {
        let is_super = matches!(
            key.code,
            KeyCode::Modifier(
                ModifierKeyCode::LeftSuper
                    | ModifierKeyCode::RightSuper
                    | ModifierKeyCode::LeftMeta
                    | ModifierKeyCode::RightMeta
            )
        );
        if !is_super {
            return false;
        }
        match key.kind {
            KeyEventKind::Press | KeyEventKind::Repeat => self.held = true,
            KeyEventKind::Release => self.held = false,
        }
        true
    }

    /// Clear the held state. Must be called on both focus loss and focus gain:
    /// when focus leaves while Super is held (e.g. Cmd+Tab away) the release
    /// event never arrives, so stale held-state would otherwise persist.
    pub(crate) fn clear(&mut self) {
        self.held = false;
    }
}

pub(crate) fn encode_key_event(
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

pub(crate) fn encode_mouse_event(
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

/// Encode pasted text for the child PTY.
///
/// When the inner terminal has bracketed paste mode enabled we wrap the data
/// in the bracketed paste sequences and apply libghostty's paste sanitization
/// (which strips unsafe control bytes). When it is disabled we forward the raw
/// bytes unchanged, preserving prior behavior.
pub(crate) fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    if !bracketed {
        return text.as_bytes().to_vec();
    }

    let mut data = text.as_bytes().to_vec();
    // Bracketed wrapping adds a fixed prefix/suffix; sanitization is 1:1 so the
    // body length is preserved. Reserve a little slack for the wrapping.
    let mut buf = vec![0_u8; data.len() + 16];
    match paste::encode(&mut data, true, &mut buf) {
        Ok(len) => {
            buf.truncate(len);
            buf
        }
        Err(_) => {
            // Fall back to a manual bracketed wrap of the raw bytes.
            let mut wrapped = Vec::with_capacity(text.len() + 12);
            wrapped.extend_from_slice(b"\x1b[200~");
            wrapped.extend_from_slice(text.as_bytes());
            wrapped.extend_from_slice(b"\x1b[201~");
            wrapped
        }
    }
}

pub(crate) fn encode_focus_event(event: ghostty_focus::Event) -> Result<Vec<u8>> {
    let mut buf = [0_u8; 8];
    let len = event.encode(&mut buf)?;
    Ok(buf[..len].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modifier_key(modifier: ModifierKeyCode, kind: KeyEventKind) -> KeyEvent {
        KeyEvent::new_with_kind(KeyCode::Modifier(modifier), KeyModifiers::SUPER, kind)
    }

    #[test]
    fn super_state_tracks_press_and_release() {
        let mut state = SuperKeyState::default();
        assert!(!state.is_held());

        assert!(state.handle_key(modifier_key(ModifierKeyCode::LeftSuper, KeyEventKind::Press)));
        assert!(state.is_held());

        assert!(state.handle_key(modifier_key(ModifierKeyCode::LeftSuper, KeyEventKind::Release)));
        assert!(!state.is_held());
    }

    #[test]
    fn super_state_tracks_meta_and_right_variants() {
        let mut state = SuperKeyState::default();
        assert!(state.handle_key(modifier_key(ModifierKeyCode::LeftMeta, KeyEventKind::Press)));
        assert!(state.is_held());
        assert!(state.handle_key(modifier_key(ModifierKeyCode::RightSuper, KeyEventKind::Release)));
        assert!(!state.is_held());
    }

    #[test]
    fn super_state_ignores_non_super_keys() {
        let mut state = SuperKeyState::default();
        let other = KeyEvent::new_with_kind(
            KeyCode::Char('a'),
            KeyModifiers::empty(),
            KeyEventKind::Press,
        );
        assert!(!state.handle_key(other));
        assert!(!state.is_held());

        // A non-super key press must not disturb a genuine held state either.
        state.handle_key(modifier_key(ModifierKeyCode::LeftSuper, KeyEventKind::Press));
        assert!(!state.handle_key(other));
        assert!(state.is_held());
    }

    #[test]
    fn super_state_clear_resets_held() {
        let mut state = SuperKeyState::default();
        state.handle_key(modifier_key(ModifierKeyCode::LeftSuper, KeyEventKind::Press));
        assert!(state.is_held());
        // Focus change: the release event never arrives, so clear() must reset.
        state.clear();
        assert!(!state.is_held());
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
    fn paste_without_bracketed_mode_is_raw() {
        assert_eq!(encode_paste("hello\nworld", false), b"hello\nworld");
    }

    #[test]
    fn paste_with_bracketed_mode_is_wrapped() {
        let out = encode_paste("hello", true);
        assert!(
            out.starts_with(b"\x1b[200~"),
            "missing bracketed paste start: {out:?}"
        );
        assert!(
            out.ends_with(b"\x1b[201~"),
            "missing bracketed paste end: {out:?}"
        );
        let body = &out[b"\x1b[200~".len()..out.len() - b"\x1b[201~".len()];
        assert_eq!(body, b"hello");
    }

    #[test]
    fn paste_with_bracketed_mode_sanitizes_end_sequence_injection() {
        // A payload containing the bracketed-paste end marker must not be able
        // to terminate paste mode early; libghostty strips the escape byte.
        let out = encode_paste("a\x1b[201~b", true);
        assert!(out.starts_with(b"\x1b[200~"));
        assert!(out.ends_with(b"\x1b[201~"));
        let body = &out[b"\x1b[200~".len()..out.len() - b"\x1b[201~".len()];
        // The interior ESC is neutralized, so no second end marker remains.
        assert!(
            !body.windows(6).any(|w| w == b"\x1b[201~"),
            "interior end marker survived: {body:?}"
        );
    }
}
