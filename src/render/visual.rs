//! VisualElement — 纯数据渲染描述
use std::sync::Arc;
use crate::geometry::Color;
pub use kurbo::{Affine, BezPath, Point as KPoint, Rect as KRect, Vec2};

pub type TextLayout = String;

#[derive(Debug, Clone, Copy, Default)]
pub struct Transform { pub translate: Vec2, pub rotate: f64, pub scale: Vec2 }
impl Transform {
    pub fn new() -> Self { Self::default() }
    pub fn translate(x: f64, y: f64) -> Self { Self { translate: Vec2::new(x, y), ..Default::default() } }
    pub fn to_affine(&self) -> Affine { Affine::translate(self.translate) * Affine::rotate(self.rotate) * Affine::scale_non_uniform(self.scale.x, self.scale.y) }
}

#[derive(Clone, Debug)]
pub struct Stroke { pub color: Color, pub width: f64 }
impl Stroke { pub fn new(color: Color, width: f64) -> Self { Self { color, width } } }
impl Default for Stroke { fn default() -> Self { Self { color: Color::BLACK, width: 1.0 } } }

#[derive(Clone, Debug, Default)]
pub struct FillStrokeStyle { pub fill: Option<Color>, pub stroke: Option<Stroke> }
impl FillStrokeStyle {
    pub fn new() -> Self { Self::default() }
    pub fn with_fill(mut self, color: Color) -> Self { self.fill = Some(color); self }
    pub fn with_stroke(mut self, color: Color, width: f64) -> Self { self.stroke = Some(Stroke::new(color, width)); self }
}

#[derive(Clone, Debug)]
pub struct LayeredElement { pub element: VisualElement, pub z_index: i32, pub element_id: Option<u64> }
impl LayeredElement {
    pub fn new(element: VisualElement, z_index: i32) -> Self { Self { element, z_index, element_id: None } }
    pub fn with_id(mut self, id: u64) -> Self { self.element_id = Some(id); self }
}

pub enum VisualElement {
    Rect { rect: KRect, style: FillStrokeStyle },
    RoundedRect { rect: KRect, radius: f64, style: FillStrokeStyle },
    Circle { center: KPoint, radius: f64, style: FillStrokeStyle },
    Line { start: KPoint, end: KPoint, style: Stroke },
    Path { path: BezPath, style: FillStrokeStyle },
    GradientPath { path: BezPath, gradient: (), stroke: Option<Stroke> },
    TextRun { text: String, position: KPoint, color: Color, font_size: f64, font_family: String, rotation: f64, max_width: Option<f64>, layout: Option<Box<TextLayout>> },
    Image { bounds: KRect, data: Arc<Vec<u8>>, width: u32, height: u32, opacity: Option<f32> },
    BoxShadow { rect: KRect, radius: f64, shadow: () },
    Group { children: Vec<LayeredElement>, transform: Option<Transform> },
}

impl std::fmt::Debug for VisualElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rect { rect, style } => f.debug_struct("Rect").field("rect", rect).field("style", style).finish(),
            Self::TextRun { text, color, font_size, .. } => f.debug_struct("TextRun").field("text", text).field("color", color).field("font_size", font_size).finish(),
            _ => write!(f, "{:?}", std::mem::discriminant(self)),
        }
    }
}

impl Clone for VisualElement {
    fn clone(&self) -> Self {
        match self {
            Self::Rect { rect, style } => Self::Rect { rect: *rect, style: style.clone() },
            Self::RoundedRect { rect, radius, style, .. } => Self::RoundedRect { rect: *rect, radius: *radius, style: style.clone() },
            Self::Circle { center, radius, style, .. } => Self::Circle { center: *center, radius: *radius, style: style.clone() },
            Self::Line { start, end, style } => Self::Line { start: *start, end: *end, style: style.clone() },
            Self::Path { path, style } => Self::Path { path: path.clone(), style: style.clone() },
            Self::GradientPath { path, stroke, .. } => Self::GradientPath { path: path.clone(), gradient: (), stroke: stroke.clone() },
            Self::TextRun { text, position, color, font_size, font_family, rotation, max_width, layout } => Self::TextRun {
                text: text.clone(), position: *position, color: *color, font_size: *font_size, font_family: font_family.clone(), rotation: *rotation, max_width: *max_width, layout: layout.clone(),
            },
            Self::Image { bounds, data, width, height, opacity } => Self::Image { bounds: *bounds, data: data.clone(), width: *width, height: *height, opacity: *opacity },
            Self::BoxShadow { rect, radius, .. } => Self::BoxShadow { rect: *rect, radius: *radius, shadow: () },
            Self::Group { children, transform } => Self::Group { children: children.clone(), transform: *transform },
        }
    }
}
