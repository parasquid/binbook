#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum FontWeight {
    #[default]
    Normal,
    Bold,
    Numeric(u16),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    #[default]
    Inline,
    Block,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputedStyle {
    pub font_family: String,
    pub font_size_milli_px: u32,
    pub font_weight: FontWeight,
    pub font_style: FontStyle,
    pub line_height_milli: u32,
    pub color_gray: u8,
    pub text_align: TextAlign,
    pub display: DisplayMode,
    pub margin_top_px: i32,
    pub margin_right_px: i32,
    pub margin_bottom_px: i32,
    pub margin_left_px: i32,
    pub padding_top_px: i32,
    pub padding_right_px: i32,
    pub padding_bottom_px: i32,
    pub padding_left_px: i32,
    pub letter_spacing_milli_em: i32,
    pub word_spacing_milli_em: i32,
    pub text_indent_px: i32,
    pub preserve_whitespace: bool,
    pub break_before: bool,
    pub break_after: bool,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        Self {
            font_family: "serif".into(),
            font_size_milli_px: 24_000,
            font_weight: FontWeight::Normal,
            font_style: FontStyle::Normal,
            line_height_milli: 1_250,
            color_gray: 0,
            text_align: TextAlign::Start,
            display: DisplayMode::Inline,
            margin_top_px: 0,
            margin_right_px: 0,
            margin_bottom_px: 0,
            margin_left_px: 0,
            padding_top_px: 0,
            padding_right_px: 0,
            padding_bottom_px: 0,
            padding_left_px: 0,
            letter_spacing_milli_em: 0,
            word_spacing_milli_em: 0,
            text_indent_px: 0,
            preserve_whitespace: false,
            break_before: false,
            break_after: false,
        }
    }
}

impl ComputedStyle {
    #[must_use]
    pub fn cascade(parent: &Self, patch: &StylePatch) -> Self {
        Self {
            font_family: patch
                .font_family
                .clone()
                .unwrap_or_else(|| parent.font_family.clone()),
            font_size_milli_px: patch
                .font_size_milli_px
                .unwrap_or(parent.font_size_milli_px),
            font_weight: patch.font_weight.unwrap_or(parent.font_weight),
            font_style: patch.font_style.unwrap_or(parent.font_style),
            line_height_milli: patch.line_height_milli.unwrap_or(parent.line_height_milli),
            color_gray: patch.color_gray.unwrap_or(parent.color_gray),
            text_align: patch.text_align.unwrap_or(parent.text_align),
            display: patch.display.unwrap_or_default(),
            margin_top_px: patch.margin_top_px.unwrap_or(0),
            margin_right_px: patch.margin_right_px.unwrap_or(0),
            margin_bottom_px: patch.margin_bottom_px.unwrap_or(0),
            margin_left_px: patch.margin_left_px.unwrap_or(0),
            padding_top_px: patch.padding_top_px.unwrap_or(0),
            padding_right_px: patch.padding_right_px.unwrap_or(0),
            padding_bottom_px: patch.padding_bottom_px.unwrap_or(0),
            padding_left_px: patch.padding_left_px.unwrap_or(0),
            letter_spacing_milli_em: patch
                .letter_spacing_milli_em
                .unwrap_or(parent.letter_spacing_milli_em),
            word_spacing_milli_em: patch
                .word_spacing_milli_em
                .unwrap_or(parent.word_spacing_milli_em),
            text_indent_px: patch.text_indent_px.unwrap_or(0),
            preserve_whitespace: patch
                .preserve_whitespace
                .unwrap_or(parent.preserve_whitespace),
            break_before: patch.break_before.unwrap_or(false),
            break_after: patch.break_after.unwrap_or(false),
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StylePatch {
    pub font_family: Option<String>,
    pub font_size_milli_px: Option<u32>,
    pub font_weight: Option<FontWeight>,
    pub font_style: Option<FontStyle>,
    pub line_height_milli: Option<u32>,
    pub color_gray: Option<u8>,
    pub text_align: Option<TextAlign>,
    pub display: Option<DisplayMode>,
    pub margin_top_px: Option<i32>,
    pub margin_right_px: Option<i32>,
    pub margin_bottom_px: Option<i32>,
    pub margin_left_px: Option<i32>,
    pub padding_top_px: Option<i32>,
    pub padding_right_px: Option<i32>,
    pub padding_bottom_px: Option<i32>,
    pub padding_left_px: Option<i32>,
    pub letter_spacing_milli_em: Option<i32>,
    pub word_spacing_milli_em: Option<i32>,
    pub text_indent_px: Option<i32>,
    pub preserve_whitespace: Option<bool>,
    pub break_before: Option<bool>,
    pub break_after: Option<bool>,
}
