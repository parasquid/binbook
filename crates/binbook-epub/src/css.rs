use binbook_document::{
    Diagnostic, DiagnosticCode, DisplayMode, FontStyle, FontWeight, StylePatch, TextAlign,
};

#[derive(Debug, Clone)]
pub(crate) struct CssRule {
    selector: String,
    patch: StylePatch,
    specificity: (u16, u16, u16),
    order: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct FontFaceRule {
    pub base: String,
    pub family: String,
    pub source: String,
    pub weight: FontWeight,
    pub style: FontStyle,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct Stylesheet {
    pub rules: Vec<CssRule>,
    pub fonts: Vec<FontFaceRule>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Stylesheet {
    pub fn extend(&mut self, mut other: Self) {
        let base = self.rules.len() as u32;
        for rule in &mut other.rules {
            rule.order += base;
        }
        self.rules.extend(other.rules);
        self.fonts.extend(other.fonts);
        self.diagnostics.extend(other.diagnostics);
    }

    pub fn patch_for(&self, tag: &str, id: Option<&str>, classes: &str) -> StylePatch {
        let mut matches: Vec<&CssRule> = self
            .rules
            .iter()
            .filter(|rule| selector_matches(&rule.selector, tag, id, classes))
            .collect();
        matches.sort_by_key(|rule| (rule.specificity, rule.order));
        let mut patch = StylePatch::default();
        for rule in matches {
            merge_patch(&mut patch, &rule.patch);
        }
        patch
    }
}

pub(crate) fn parse_css(source: &str, resource: &str) -> Stylesheet {
    let mut stylesheet = Stylesheet::default();
    let mut order = 0_u32;
    for segment in source.split('}') {
        let Some((selector, declarations)) = segment.rsplit_once('{') else {
            continue;
        };
        let selector = selector.trim();
        if selector.is_empty() {
            continue;
        }
        if selector.ends_with("@font-face") || selector == "@font-face" {
            if let Some(font) = parse_font_face(declarations, resource) {
                stylesheet.fonts.push(font);
            }
            continue;
        }
        let mut patch = StylePatch::default();
        parse_declarations(
            declarations,
            &mut patch,
            resource,
            &mut stylesheet.diagnostics,
        );
        for selector in selector
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            stylesheet.rules.push(CssRule {
                selector: selector.into(),
                patch: patch.clone(),
                specificity: specificity(selector),
                order,
            });
            order = order.saturating_add(1);
        }
    }
    stylesheet
}

pub(crate) fn parse_inline(
    source: &str,
    resource: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> StylePatch {
    let mut patch = StylePatch::default();
    parse_declarations(source, &mut patch, resource, diagnostics);
    patch
}

fn parse_declarations(
    source: &str,
    patch: &mut StylePatch,
    resource: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for declaration in source.split(';') {
        let Some((name, value)) = declaration.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().trim_matches('"').trim_matches('\'');
        cssparser::match_ignore_ascii_case! { &name,
            "font-family" => patch.font_family = Some(value.split(',').next().unwrap_or(value).trim_matches('"').trim_matches('\'').into()),
            "font-size" => patch.font_size_milli_px = parse_px(value).map(|value| value as u32 * 1_000),
            "font-weight" => patch.font_weight = Some(parse_weight(value)),
            "font-style" => patch.font_style = Some(if value.eq_ignore_ascii_case("italic") { FontStyle::Italic } else if value.eq_ignore_ascii_case("oblique") { FontStyle::Oblique } else { FontStyle::Normal }),
            "line-height" => patch.line_height_milli = parse_milli(value),
            "display" => patch.display = Some(if value.eq_ignore_ascii_case("none") { DisplayMode::None } else if value.eq_ignore_ascii_case("block") { DisplayMode::Block } else { DisplayMode::Inline }),
            "text-align" => patch.text_align = Some(match value { "center" => TextAlign::Center, "right" | "end" => TextAlign::End, "justify" => TextAlign::Justify, _ => TextAlign::Start }),
            "margin-top" => patch.margin_top_px = parse_px(value),
            "margin-right" => patch.margin_right_px = parse_px(value),
            "margin-bottom" => patch.margin_bottom_px = parse_px(value),
            "margin-left" => patch.margin_left_px = parse_px(value),
            "padding-top" => patch.padding_top_px = parse_px(value),
            "padding-right" => patch.padding_right_px = parse_px(value),
            "padding-bottom" => patch.padding_bottom_px = parse_px(value),
            "padding-left" => patch.padding_left_px = parse_px(value),
            "letter-spacing" => patch.letter_spacing_milli_em = parse_em(value),
            "word-spacing" => patch.word_spacing_milli_em = parse_em(value),
            "text-indent" => patch.text_indent_px = parse_px(value),
            "white-space" => patch.preserve_whitespace = Some(matches!(value, "pre" | "pre-wrap")),
            "break-before" | "page-break-before" => patch.break_before = Some(matches!(value, "page" | "always")),
            "break-after" | "page-break-after" => patch.break_after = Some(matches!(value, "page" | "always")),
            "float" | "position" | "columns" | "column-count" | "grid" | "flex" => diagnostics.push(Diagnostic::new(DiagnosticCode::UnsupportedCss, resource, 0)),
            _ => {}
        }
    }
}

fn parse_font_face(source: &str, resource: &str) -> Option<FontFaceRule> {
    let mut family = None;
    let mut path = None;
    let mut weight = FontWeight::Normal;
    let mut style = FontStyle::Normal;
    for declaration in source.split(';') {
        let Some((name, value)) = declaration.split_once(':') else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'');
        match name.trim() {
            "font-family" => family = Some(value.into()),
            "src" => {
                path = value
                    .split("url(")
                    .nth(1)
                    .and_then(|value| value.split(')').next())
                    .map(|value| value.trim_matches('"').trim_matches('\'').into())
            }
            "font-weight" => weight = parse_weight(value),
            "font-style" if value == "italic" => style = FontStyle::Italic,
            "font-style" if value == "oblique" => style = FontStyle::Oblique,
            _ => {}
        }
    }
    Some(FontFaceRule {
        base: resource.into(),
        family: family?,
        source: path?,
        weight,
        style,
    })
}

fn selector_matches(selector: &str, tag: &str, id: Option<&str>, classes: &str) -> bool {
    let selector = selector.split_whitespace().last().unwrap_or(selector);
    if let Some(expected) = selector.strip_prefix('#') {
        return id == Some(expected);
    }
    if let Some(expected) = selector.strip_prefix('.') {
        return classes.split_whitespace().any(|class| class == expected);
    }
    selector.eq_ignore_ascii_case(tag)
}

fn specificity(selector: &str) -> (u16, u16, u16) {
    (
        selector.matches('#').count() as u16,
        selector.matches('.').count() as u16,
        u16::from(!selector.starts_with(['#', '.'])),
    )
}

pub(crate) fn merge_patch(target: &mut StylePatch, source: &StylePatch) {
    macro_rules! copy { ($($field:ident),* $(,)?) => { $(if source.$field.is_some() { target.$field = source.$field.clone(); })* }; }
    copy!(
        font_family,
        font_size_milli_px,
        font_weight,
        font_style,
        line_height_milli,
        color_gray,
        text_align,
        display,
        margin_top_px,
        margin_right_px,
        margin_bottom_px,
        margin_left_px,
        padding_top_px,
        padding_right_px,
        padding_bottom_px,
        padding_left_px,
        letter_spacing_milli_em,
        word_spacing_milli_em,
        text_indent_px,
        preserve_whitespace,
        break_before,
        break_after
    );
}

fn parse_px(value: &str) -> Option<i32> {
    value.trim_end_matches("px").parse().ok()
}
fn parse_em(value: &str) -> Option<i32> {
    value
        .trim_end_matches("em")
        .parse::<f32>()
        .ok()
        .map(|value| (value * 1_000.0).round() as i32)
}
fn parse_milli(value: &str) -> Option<u32> {
    value
        .parse::<f32>()
        .ok()
        .map(|value| (value * 1_000.0).round() as u32)
}
fn parse_weight(value: &str) -> FontWeight {
    if value.eq_ignore_ascii_case("bold") {
        FontWeight::Bold
    } else {
        value
            .parse()
            .map(FontWeight::Numeric)
            .unwrap_or(FontWeight::Normal)
    }
}
