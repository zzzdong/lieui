use quick_xml::Reader;
use quick_xml::events::attributes::AttrError;
use quick_xml::events::{Event, attributes::Attribute};
use taffy::{
    Dimension, Display, LengthPercentage, LengthPercentageAuto, Rect as TaffyRect,
    Size as TaffySize,
};
use thiserror::Error;
use vello_cpu::peniko::Color;

#[derive(Error, Debug)]
pub enum StyleError {
    #[error("XML error: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Bad attribute: {0}")]
    Attr(String),
    #[error("Bad color: {0}")]
    Color(String),
    #[error("Parse int error: {0}")]
    ParseInt(#[from] std::num::ParseIntError),
    #[error("Parse float error: {0}")]
    ParseFloat(#[from] std::num::ParseFloatError),
    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("Encoding error: {0}")]
    Encoding(#[from] quick_xml::encoding::EncodingError),
    #[error("Attribute error: {0}")]
    Attribute(#[from] AttrError),
    #[error("{0}")]
    Message(String),
}

type Result<T> = std::result::Result<T, StyleError>;

#[derive(Debug, Clone)]
pub struct Style {
    pub display: Display,
    pub size: TaffySize<Dimension>,
    pub min_size: TaffySize<Dimension>,
    pub max_size: TaffySize<Dimension>,
    pub margin: TaffyRect<LengthPercentageAuto>,
    pub padding: TaffyRect<LengthPercentage>,
    pub border: TaffyRect<LengthPercentage>,
    pub border_radius: f64,
    pub background_color: Color,
    pub border_color: Color,
    pub color: Color,
    pub font_size: f64,
    pub font_family: String,
}

impl Style {
    pub fn from_attrs(attrs: Vec<Attribute>) -> Result<Self> {
        let mut style = Style::default();

        let mut margin = LengthPercentageAuto::auto();
        let mut padding = LengthPercentage::length(0.0);
        let mut boarder = LengthPercentage::length(0.0);

        for attr in attrs {
            let key = std::str::from_utf8(attr.key.as_ref())?.to_lowercase();
            let val = std::str::from_utf8(&attr.value)?;
            match key.as_str() {
                "display" => {
                    style.display = match val {
                        "flex" => Display::Flex,
                        "block" => Display::Block,
                        "grid" => Display::Grid,
                        "none" => Display::None,
                        _ => return Err(StyleError::Message(format!("unknown display: {}", val))),
                    };
                }
                // Dimension 系列
                "width" => style.size.width = parse_dimension(val)?,
                "height" => style.size.height = parse_dimension(val)?,
                "min-width" => style.min_size.width = parse_dimension(val)?,
                "min-height" => style.min_size.height = parse_dimension(val)?,
                "max-width" => style.max_size.width = parse_dimension(val)?,
                "max-height" => style.max_size.height = parse_dimension(val)?,

                // LengthPercentage 系列
                "boarder" => boarder = parse_length_percent(val)?,
                "border-left" => style.border.left = parse_length_percent(val)?,
                "border-top" => style.border.top = parse_length_percent(val)?,
                "border-right" => style.border.right = parse_length_percent(val)?,
                "border-bottom" => style.border.bottom = parse_length_percent(val)?,
                "padding" => padding = parse_length_percent(val)?,
                "padding-left" => style.padding.left = parse_length_percent(val)?,
                "padding-top" => style.padding.top = parse_length_percent(val)?,
                "padding-right" => style.padding.right = parse_length_percent(val)?,
                "padding-bottom" => style.padding.bottom = parse_length_percent(val)?,

                // LengthPercentageAuto 系列
                "margin" => margin = parse_length_percent_auto(val)?,
                "margin-left" => style.margin.left = parse_length_percent_auto(val)?,
                "margin-top" => style.margin.top = parse_length_percent_auto(val)?,
                "margin-right" => style.margin.right = parse_length_percent_auto(val)?,
                "margin-bottom" => style.margin.bottom = parse_length_percent_auto(val)?,

                // 单值
                "border-radius" => style.border_radius = val.parse()?,
                "font-size" => style.font_size = val.parse()?,

                // Color 系列
                "border-color" => style.border_color = parse_color(val)?,
                "background-color" => style.background_color = parse_color(val)?,
                "color" => style.color = parse_color(val)?,

                // String 系列
                "font-family" => style.font_family = val.into(),

                _ => {} // 忽略不认识
            }
        }

        // 展开缩写：单边没写就用统一的值
        if margin.is_auto() {
            style.margin.left = margin;
            style.margin.top = margin;
            style.margin.right = margin;
            style.margin.bottom = margin;
        }
        if padding == LengthPercentage::length(0.0) {
            style.padding.left = padding;
            style.padding.top = padding;
            style.padding.right = padding;
            style.padding.bottom = padding;
        }
        if boarder == LengthPercentage::length(0.0) {
            style.border.left = boarder;
            style.border.top = boarder;
            style.border.right = boarder;
            style.border.bottom = boarder;
        }

        Ok(style)
    }
}

impl Default for Style {
    fn default() -> Self {
        Style {
            display: Display::Flex,
            size: TaffySize::auto(),
            min_size: TaffySize::auto(),
            max_size: TaffySize::auto(),
            margin: TaffyRect::zero(),
            padding: TaffyRect::zero(),
            border: TaffyRect::zero(),
            border_radius: 0.0,
            background_color: Color::BLACK,
            border_color: Color::TRANSPARENT,
            color: Color::BLACK,
            font_size: 12.0,
            font_family: "sans-serif".into(),
        }
    }
}

fn parse_dimension(s: &str) -> Result<Dimension> {
    let s = s.trim();
    if s == "auto" {
        return Ok(Dimension::auto());
    }
    if s.ends_with('%') {
        let num: f32 = s[..s.len() - 1]
            .parse()
            .map_err(|_| StyleError::Message("invalid percent".into()))?;
        return Ok(Dimension::percent(num / 100.0));
    }
    if s.ends_with("fr") {
        return Err(StyleError::Message("flex fr unit not supported yet".into()));
    }
    // 默认当成 px
    let px: f32 = s
        .parse()
        .map_err(|_| StyleError::Message("invalid length".into()))?;
    Ok(Dimension::length(px))
}

fn parse_length_percent(s: &str) -> Result<LengthPercentage> {
    let s = s.trim();

    if s.ends_with('%') {
        let num: f32 = s[..s.len() - 1]
            .parse()
            .map_err(|_| StyleError::Message("invalid percent".into()))?;
        return Ok(LengthPercentage::percent(num / 100.0));
    }

    let px: f32 = s
        .parse()
        .map_err(|_| StyleError::Message("invalid length".into()))?;
    return Ok(LengthPercentage::length(px));
}

fn parse_length_percent_auto(s: &str) -> Result<LengthPercentageAuto> {
    let s = s.trim();

    if s == "auto" {
        return Ok(LengthPercentageAuto::auto());
    }

    if s.ends_with('%') {
        let num: f32 = s[..s.len() - 1]
            .parse()
            .map_err(|_| StyleError::Message("invalid percent".into()))?;
        return Ok(LengthPercentageAuto::percent(num / 100.0));
    }

    let px: f32 = s
        .parse()
        .map_err(|_| StyleError::Message("invalid length".into()))?;
    return Ok(LengthPercentageAuto::length(px));
}

fn parse_color(s: &str) -> Result<Color> {
    let s = s.trim();

    if s.is_empty() {
        return Ok(Color::TRANSPARENT);
    }

    if s.starts_with('#') && s.len() == 7 {
        let r = u8::from_str_radix(&s[1..3], 16)?;
        let g = u8::from_str_radix(&s[3..5], 16)?;
        let b = u8::from_str_radix(&s[5..7], 16)?;
        return Ok(Color::from_rgb8(r, g, b));
    }
    if s.starts_with('#') && s.len() == 4 {
        let r = u8::from_str_radix(&s[1..2], 16)? * 17;
        let g = u8::from_str_radix(&s[2..3], 16)? * 17;
        let b = u8::from_str_radix(&s[3..4], 16)? * 17;
        return Ok(Color::from_rgb8(r, g, b));
    }
    match s.to_lowercase().as_str() {
        "red" => Ok(Color::from_rgb8(255, 0, 0)),
        "white" => Ok(Color::from_rgb8(255, 255, 255)),
        "black" => Ok(Color::from_rgb8(0, 0, 0)),
        "transparent" => Ok(Color::TRANSPARENT),
        _ => Err(StyleError::Color(s.into())),
    }
}
