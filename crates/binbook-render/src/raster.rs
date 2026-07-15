use binbook_document::Resource;
use binbook_encode::CompiledPage;
use binbook_image::{
    compile_decoded_image, decode_image, CompileOptions, DecodedImage, ImageFormat,
};
use cosmic_text::{
    Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, Style, SwashCache, Weight,
};
use image::{imageops::FilterType, RgbaImage};

use crate::document::RenderOptions;
use crate::pagination::Page;
use crate::RenderError;

#[cfg(test)]
mod tests;

const WIDTH: u32 = 960;
const HEIGHT: u32 = 1_600;
const MARGIN: i32 = 48;

pub(crate) fn raster_page(
    page: &Page,
    fonts: &mut FontSystem,
    options: &RenderOptions,
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
    let page = compile_decoded_image(&decoded, compile_options(options))
        .map_err(|_| RenderError::Raster)?;
    Ok((page, missing_glyph))
}

pub(crate) fn raster_artwork(
    resource: &Resource,
    caption: Option<&str>,
    fonts: &mut FontSystem,
    options: &RenderOptions,
) -> Result<(Vec<CompiledPage>, bool), RenderError> {
    let decoded = decode_image(&resource.bytes).map_err(|_| RenderError::Raster)?;
    let panels = if decoded.width > decoded.height { 2 } else { 1 };
    let source = RgbaImage::from_raw(decoded.width, decoded.height, decoded.rgba)
        .ok_or(RenderError::Raster)?;
    let mut pages = Vec::with_capacity(panels);
    let mut caption_truncated = false;
    for panel in 0..panels {
        let (page, truncated) = compose_artwork(&source, panel, panels, caption, fonts, options)?;
        pages.push(page);
        caption_truncated |= truncated;
    }
    Ok((pages, caption_truncated))
}

fn compose_artwork(
    source: &RgbaImage,
    panel: usize,
    panels: usize,
    caption: Option<&str>,
    fonts: &mut FontSystem,
    options: &RenderOptions,
) -> Result<(CompiledPage, bool), RenderError> {
    let (source_x, source_width) = panel_bounds(source.width(), panel, panels);
    let source =
        image::imageops::crop_imm(source, source_x, 0, source_width, source.height()).to_image();
    let margin = if caption.is_some() { 48 } else { 0 };
    let caption_height = if caption.is_some() { 176 } else { 0 };
    let target_width = WIDTH - margin * 2;
    let target_height = HEIGHT - margin * 2 - caption_height;
    let scale = (target_width as f64 / source.width() as f64)
        .min(target_height as f64 / source.height() as f64);
    let fitted_width = (source.width() as f64 * scale).round().max(1.0) as u32;
    let fitted_height = (source.height() as f64 * scale).round().max(1.0) as u32;
    let resized =
        image::imageops::resize(&source, fitted_width, fitted_height, FilterType::Lanczos3);
    let mut canvas = RgbaImage::from_pixel(WIDTH, HEIGHT, image::Rgba([255; 4]));
    let x = margin + (target_width - fitted_width) / 2;
    let y = margin + (target_height - fitted_height) / 2;
    image::imageops::overlay(&mut canvas, &resized, i64::from(x), i64::from(y));
    let truncated = caption.is_some_and(|value| {
        draw_caption(
            canvas.as_mut(),
            value,
            fonts,
            margin as i32,
            (HEIGHT - margin - caption_height) as i32,
            target_width,
            caption_height,
        )
    });
    let composed = DecodedImage {
        format: ImageFormat::Png,
        width: WIDTH,
        height: HEIGHT,
        rgba: canvas.into_raw(),
    };
    let page = compile_decoded_image(&composed, compile_options(options))
        .map_err(|_| RenderError::Raster)?;
    Ok((page, truncated))
}

fn panel_bounds(width: u32, panel: usize, panels: usize) -> (u32, u32) {
    if panels == 1 {
        return (0, width);
    }
    let left_width = width.div_ceil(2);
    if panel == 0 {
        (0, left_width)
    } else {
        (left_width, width - left_width)
    }
}

fn draw_caption(
    rgba: &mut [u8],
    caption: &str,
    fonts: &mut FontSystem,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> bool {
    let (text, truncated) = fitting_caption(caption, fonts, width, height);
    let mut buffer = caption_buffer(fonts, &text, width, height);
    let mut cache = SwashCache::new();
    buffer.draw(
        fonts,
        &mut cache,
        Color::rgb(0, 0, 0),
        |px, py, w, h, color| {
            blend(rgba, px + x, py + y, w, h, color.a());
        },
    );
    truncated
}

fn fitting_caption(
    caption: &str,
    fonts: &mut FontSystem,
    width: u32,
    height: u32,
) -> (String, bool) {
    if caption_line_count(caption, fonts, width, height) <= 4 {
        return (caption.into(), false);
    }
    let mut boundary = caption.len();
    loop {
        boundary = floor_char_boundary(caption, boundary.saturating_sub(1));
        let candidate = format!("{}…", caption[..boundary].trim_end());
        if caption_line_count(&candidate, fonts, width, height) <= 4 || boundary == 0 {
            return (candidate, true);
        }
    }
}

fn caption_line_count(text: &str, fonts: &mut FontSystem, width: u32, height: u32) -> usize {
    let mut buffer = caption_buffer(fonts, text, width, height);
    buffer.shape_until_scroll(fonts, false);
    buffer.layout_runs().count()
}

fn caption_buffer(fonts: &mut FontSystem, text: &str, width: u32, height: u32) -> Buffer {
    let mut buffer = Buffer::new(fonts, Metrics::new(36.0, 44.0));
    buffer.set_size(Some(width as f32), Some(height as f32));
    buffer.set_text(text, &Attrs::new(), Shaping::Advanced, None);
    buffer
}

fn floor_char_boundary(value: &str, mut index: usize) -> usize {
    while index > 0 && !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn compile_options(options: &RenderOptions) -> CompileOptions {
    CompileOptions {
        storage_format: options.storage_format,
        dither: options.dither,
    }
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
