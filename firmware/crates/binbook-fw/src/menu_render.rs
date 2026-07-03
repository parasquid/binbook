use embedded_graphics::{
    geometry::{Point, Size},
    mono_font::MonoTextStyleBuilder,
    prelude::{Drawable, Primitive},
    primitives::{PrimitiveStyleBuilder, Rectangle},
    text::Text,
};
use xteink_x4_display::{
    framebuffer::{Gray2Color, Gray2Framebuffer},
    profile::LOGICAL_WIDTH,
};

use crate::menu::{MenuNames, MenuState};

/// Vertical pixel offset for the first menu row.
pub const MENU_ROW_START_Y: i32 = 76;
/// Vertical stride between consecutive menu rows.
pub const MENU_ROW_STRIDE: i32 = 96;
/// Padding from the left edge for entry text.
pub const MENU_TEXT_LEFT: i32 = 16;
/// Horizontal padding from the right edge for page-count suffix.
pub const MENU_TEXT_RIGHT: i32 = 16;
/// Thickness of the selection highlight stroke.
pub const HIGHLIGHT_STROKE: u32 = 4;

/// Render the current menu viewport into `fb`.
///
/// Clears the framebuffer to white, draws the visible entry names in black
/// text centered in each row, and strokes a `HIGHLIGHT_STROKE`-pixel rectangle
/// around the selected row.
pub fn render_menu(fb: &mut Gray2Framebuffer, state: &MenuState, names: &MenuNames) {
    fb.clear(Gray2Color::WHITE);

    let visible = state.visible_count();
    for row in 0..visible {
        let index = state.top() + row;
        let name = names.get(index).map_or_else(|| "?", |n| n.as_str());
        let y = MENU_ROW_START_Y + row as i32 * MENU_ROW_STRIDE;
        let selected = index == state.selected();

        if selected {
            let rect = Rectangle::new(
                Point::new(HIGHLIGHT_STROKE as i32, y),
                Size::new(
                    (LOGICAL_WIDTH as i32 - 2 * HIGHLIGHT_STROKE as i32) as u32,
                    MENU_ROW_STRIDE as u32 - 2 * HIGHLIGHT_STROKE,
                ),
            );
            let _ = rect
                .into_styled(
                    PrimitiveStyleBuilder::new()
                        .fill_color(Gray2Color::BLACK)
                        .build(),
                )
                .draw(fb);
        }

        if !name.is_empty() {
            let style = MonoTextStyleBuilder::new()
                .font(&profont::PROFONT_24_POINT)
                .text_color(if selected {
                    Gray2Color::WHITE
                } else {
                    Gray2Color::BLACK
                })
                .build();
            let _ = Text::new(name, Point::new(MENU_TEXT_LEFT, y + 36), style).draw(fb);
        }
    }
}
