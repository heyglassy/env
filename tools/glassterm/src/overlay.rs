//! Theme-derived styling for glassterm's interaction affordances.
//!
//! Text selection is now driven natively by libghostty-vt (see
//! [`crate::selection`]) and painted by the renderer from the render
//! snapshot, so this module no longer tracks selection rectangles. It only
//! carries the resolved selection colors from the theme to the renderer.

use libghostty_vt::style::RgbColor;

use crate::theme::Theme;

#[derive(Debug, Clone, Copy)]
pub(crate) struct InteractionStyle {
    pub(crate) foreground: RgbColor,
    pub(crate) background: RgbColor,
}

impl Default for InteractionStyle {
    fn default() -> Self {
        let theme = Theme::default();
        theme.interaction_style()
    }
}
