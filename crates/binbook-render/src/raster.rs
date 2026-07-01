use binbook_encode::CompiledPage;
use binbook_image::{compile_decoded_image, CompileOptions, DecodedImage, ImageFormat};
use cosmic_text::{
    Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, Style, SwashCache, Weight,
};

use crate::pagination::Page;
use crate::RenderError;

const WIDTH: u32 = 960;
const HEIGHT: u32 = 1_600;
const MARGIN: i32 = 48;

pub(crate) fn raster_page(
    page: &Page,
    fonts: &mut FontSystem,
) -> Result<(CompiledPage, bool), RenderError> {
    let mut rgba = vec![255_u8; (WIDTH * HEIGHT * 4) as usize];
    let mut buffer = Buffer::new(fonts, Metrics::new(42.0, 54.0));
    buffer.set_size(Some((WIDTH - 96) as f32), Some((HEIGHT - 96) as f32));
    let rich = page.spans.iter().map(|span| {
        let weight = match span.style.font_weight {
            binbook_document::FontWeight::Normal => Weight::NORMAL,
            binbook_document::FontWeight::Bold => Weight::BOLD,
            binbook_document::FontWeight::Numeric(value) => Weight(value),
        };
        let style = match span.style.font_style {
            binbook_document::FontStyle::Normal => Style::Normal,
            binbook_document::FontStyle::Italic => Style::Italic,
            binbook_document::FontStyle::Oblique => Style::Oblique,
        };
        (
            span.text.as_str(),
            Attrs::new()
                .family(Family::Name(&span.style.font_family))
                .weight(weight)
                .style(style),
        )
    });
    buffer.set_rich_text(rich, &Attrs::new(), Shaping::Advanced, None);
    buffer.shape_until_scroll(fonts, false);
    let missing_glyph = buffer
        .layout_runs()
        .flat_map(|run| run.glyphs)
        .any(|glyph| glyph.glyph_id == 0);
    let mut cache = SwashCache::new();
    buffer.draw(
        fonts,
        &mut cache,
        Color::rgb(0, 0, 0),
        |x, y, width, height, color| {
            blend(&mut rgba, x + MARGIN, y + MARGIN, width, height, color.a());
        },
    );
    let decoded = DecodedImage {
        format: ImageFormat::Png,
        width: WIDTH,
        height: HEIGHT,
        rgba,
    };
    let page = compile_decoded_image(&decoded, CompileOptions::default())
        .map_err(|_| RenderError::Raster)?;
    Ok((page, missing_glyph))
}

fn blend(target: &mut [u8], x: i32, y: i32, width: u32, height: u32, alpha: u8) {
    for row in 0..height as i32 {
        for column in 0..width as i32 {
            let px = x + column;
            let py = y + row;
            if px < 0 || py < 0 || px >= WIDTH as i32 || py >= HEIGHT as i32 {
                continue;
            }
            let index = ((py as u32 * WIDTH + px as u32) * 4) as usize;
            let value = 255_u8.saturating_sub(alpha);
            target[index..index + 3].fill(value);
            target[index + 3] = 255;
        }
    }
}
