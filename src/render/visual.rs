//! 视觉元素模块 - 纯数据描述，与渲染后端解耦
//!
//! 设计原则：
//! - 纯数据，不包含任何渲染逻辑
//! - 使用标准几何类型（来自 kurbo）
//! - 支持嵌套组合（Group）
//! - 支持自定义扩展（Custom）

use crate::geometry::{Color, Rect};
use crate::text::TextLayout;
use kurbo::{BezPath, Point, Rect as KurboRect, Vec2};

/// 2D 变换
#[derive(Clone, Copy, Debug, Default)]
pub struct Transform {
    pub translate: Vec2,
    pub rotate: f64, // 弧度
    pub scale: Vec2,
}

impl Transform {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_translation(x: f64, y: f64) -> Self {
        Self {
            translate: Vec2::new(x, y),
            ..Default::default()
        }
    }

    pub fn with_rotation(angle: f64) -> Self {
        Self {
            rotate: angle,
            ..Default::default()
        }
    }

    pub fn with_scale(x: f64, y: f64) -> Self {
        Self {
            scale: Vec2::new(x, y),
            ..Default::default()
        }
    }

    /// 转换为 kurbo::Affine
    pub fn to_affine(&self) -> kurbo::Affine {
        kurbo::Affine::translate(self.translate)
            * kurbo::Affine::rotate(self.rotate)
            * kurbo::Affine::scale_non_uniform(self.scale.x, self.scale.y)
    }
}

/// 渐变定义
#[derive(Clone, Debug)]
pub struct GradientDef {
    /// 渐变停止点列表 (offset 0.0~1.0, color)
    pub stops: Vec<(f64, Color)>,
}

impl GradientDef {
    pub fn new(stops: Vec<(f64, Color)>) -> Self {
        Self { stops }
    }
}

/// 描边样式
#[derive(Clone, Debug)]
pub struct Stroke {
    pub color: Color,
    pub width: f64,
}

impl Stroke {
    pub fn new(color: Color, width: f64) -> Self {
        Self { color, width }
    }
}

impl Default for Stroke {
    fn default() -> Self {
        Self {
            color: Color::from_rgb8(0, 0, 0),
            width: 1.0,
        }
    }
}

/// 填充和描边组合样式
#[derive(Clone, Debug, Default)]
pub struct FillStrokeStyle {
    pub fill: Option<Color>,
    pub stroke: Option<Stroke>,
}

impl FillStrokeStyle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_fill(mut self, color: Color) -> Self {
        self.fill = Some(color);
        self
    }

    pub fn with_stroke(mut self, color: Color, width: f64) -> Self {
        self.stroke = Some(Stroke::new(color, width));
        self
    }
}

/// 带层级信息的视觉元素
///
/// 使用此结构体包装 VisualElement，添加 z_index 支持层级渲染和事件派发
#[derive(Debug, Clone)]
pub struct LayeredElement {
    /// 视觉元素
    pub element: VisualElement,
    /// 层级，数值越大越在上层（默认 0）
    pub z_index: i32,
}

impl LayeredElement {
    /// 创建新的带层级元素
    pub fn new(element: VisualElement, z_index: i32) -> Self {
        Self { element, z_index }
    }

    /// 使用默认层级 (0) 创建
    pub fn default_layer(element: VisualElement) -> Self {
        Self {
            element,
            z_index: 0,
        }
    }
}

/// 视觉元素枚举 - 纯数据描述，可被任何渲染后端解释
pub enum VisualElement {
    // ---- 基础图形 ----
    /// 矩形
    Rect {
        rect: KurboRect,
        style: FillStrokeStyle,
    },
    /// 圆角矩形
    RoundedRect {
        rect: KurboRect,
        radius: f64,
        style: FillStrokeStyle,
    },
    /// 圆形
    Circle {
        center: Point,
        radius: f64,
        style: FillStrokeStyle,
    },
    /// 线条
    Line {
        start: Point,
        end: Point,
        style: Stroke,
    },
    /// 折线
    Polyline { points: Vec<Point>, style: Stroke },
    /// 路径
    Path {
        path: BezPath,
        style: FillStrokeStyle,
    },

    // ---- 渐变路径 ----
    GradientPath {
        path: BezPath,
        gradient: GradientDef,
        stroke: Option<Stroke>,
    },

    // ---- 文本 ----
    TextRun {
        text: String,
        position: Point,
        color: Color,
        font_size: f64,
        font_family: String,
        rotation: f64,
        max_width: Option<f64>,
        layout: Option<TextLayout>,
    },

    // ---- 图片 ----
    Image {
        bounds: KurboRect,
        data: Vec<u8>,
        width: u32,
        height: u32,
        opacity: Option<f32>,
    },

    // ---- 阴影 ----
    BoxShadow {
        rect: KurboRect,
        radius: f64,
        shadow: BoxShadowDef,
    },

    // ---- 变换组合 ----
    Group {
        children: Vec<LayeredElement>, // 使用 LayeredElement 以支持嵌套层级
        transform: Option<Transform>,
    },
}

/// 盒阴影定义
#[derive(Clone, Debug)]
pub struct BoxShadowDef {
    pub offset_x: f64,
    pub offset_y: f64,
    pub blur_radius: f64,
    pub spread_radius: f64,
    pub color: Color,
}

impl BoxShadowDef {
    pub fn new(color: Color) -> Self {
        Self {
            offset_x: 0.0,
            offset_y: 2.0,
            blur_radius: 4.0,
            spread_radius: 0.0,
            color,
        }
    }
}

/// 手动实现 Debug for VisualElement（因为 TextLayout 没有实现 Debug）
impl std::fmt::Debug for VisualElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VisualElement::Rect { rect, style } => f
                .debug_struct("Rect")
                .field("rect", rect)
                .field("style", style)
                .finish(),
            VisualElement::RoundedRect {
                rect,
                radius,
                style,
            } => f
                .debug_struct("RoundedRect")
                .field("rect", rect)
                .field("radius", radius)
                .field("style", style)
                .finish(),
            VisualElement::Circle {
                center,
                radius,
                style,
            } => f
                .debug_struct("Circle")
                .field("center", center)
                .field("radius", radius)
                .field("style", style)
                .finish(),
            VisualElement::Line { start, end, style } => f
                .debug_struct("Line")
                .field("start", start)
                .field("end", end)
                .field("style", style)
                .finish(),
            VisualElement::Polyline { points, style } => f
                .debug_struct("Polyline")
                .field("points", points)
                .field("style", style)
                .finish(),
            VisualElement::Path { path: _, style } => f
                .debug_struct("Path")
                .field("path", &"<BezPath>")
                .field("style", style)
                .finish(),
            VisualElement::GradientPath {
                path: _,
                gradient,
                stroke,
            } => f
                .debug_struct("GradientPath")
                .field("path", &"<BezPath>")
                .field("gradient", gradient)
                .field("stroke", stroke)
                .finish(),
            VisualElement::TextRun {
                text,
                position,
                color,
                font_size,
                font_family,
                rotation,
                max_width,
                layout,
            } => f
                .debug_struct("TextRun")
                .field("text", text)
                .field("position", position)
                .field("color", color)
                .field("font_size", font_size)
                .field("font_family", font_family)
                .field("rotation", rotation)
                .field("max_width", max_width)
                .field("layout", &layout.as_ref().map(|_| "<TextLayout>"))
                .finish(),
            VisualElement::Image {
                bounds,
                width,
                height,
                opacity,
                ..
            } => f
                .debug_struct("Image")
                .field("bounds", bounds)
                .field("width", width)
                .field("height", height)
                .field("opacity", opacity)
                .finish(),
            VisualElement::BoxShadow {
                rect,
                radius,
                shadow,
            } => f
                .debug_struct("BoxShadow")
                .field("rect", rect)
                .field("radius", radius)
                .field("shadow", shadow)
                .finish(),
            VisualElement::Group {
                children,
                transform,
            } => f
                .debug_struct("Group")
                .field("children", children)
                .field("transform", transform)
                .finish(),
        }
    }
}

/// 手动实现 Clone for VisualElement（因为 TextLayout 的 clone 可能有问题）
impl Clone for VisualElement {
    fn clone(&self) -> Self {
        match self {
            VisualElement::Rect { rect, style } => VisualElement::Rect {
                rect: *rect,
                style: style.clone(),
            },
            VisualElement::RoundedRect {
                rect,
                radius,
                style,
            } => VisualElement::RoundedRect {
                rect: *rect,
                radius: *radius,
                style: style.clone(),
            },
            VisualElement::Circle {
                center,
                radius,
                style,
            } => VisualElement::Circle {
                center: *center,
                radius: *radius,
                style: style.clone(),
            },
            VisualElement::Line { start, end, style } => VisualElement::Line {
                start: *start,
                end: *end,
                style: style.clone(),
            },
            VisualElement::Polyline { points, style } => VisualElement::Polyline {
                points: points.clone(),
                style: style.clone(),
            },
            VisualElement::Path { path, style } => VisualElement::Path {
                path: path.clone(),
                style: style.clone(),
            },
            VisualElement::GradientPath {
                path,
                gradient,
                stroke,
            } => VisualElement::GradientPath {
                path: path.clone(),
                gradient: gradient.clone(),
                stroke: stroke.clone(),
            },
            VisualElement::TextRun {
                text,
                position,
                color,
                font_size,
                font_family,
                rotation,
                max_width,
                layout,
            } => VisualElement::TextRun {
                text: text.clone(),
                position: *position,
                color: *color,
                font_size: *font_size,
                font_family: font_family.clone(),
                rotation: *rotation,
                max_width: *max_width,
                layout: layout.clone(),
            },
            VisualElement::Image {
                bounds,
                data,
                width,
                height,
                opacity,
            } => VisualElement::Image {
                bounds: *bounds,
                data: data.clone(),
                width: *width,
                height: *height,
                opacity: *opacity,
            },
            VisualElement::BoxShadow {
                rect,
                radius,
                shadow,
            } => VisualElement::BoxShadow {
                rect: *rect,
                radius: *radius,
                shadow: shadow.clone(),
            },
            VisualElement::Group {
                children,
                transform,
            } => VisualElement::Group {
                children: children.clone(),
                transform: *transform,
            },
        }
    }
}

/// 描边样式（简化版，用于 Line/Polyline）
#[derive(Clone, Debug)]
pub struct StrokeStyle {
    pub color: Color,
    pub width: f64,
}

impl StrokeStyle {
    pub fn new(color: Color, width: f64) -> Self {
        Self { color, width }
    }
}

impl Default for StrokeStyle {
    fn default() -> Self {
        Self {
            color: Color::from_rgb8(0, 0, 0),
            width: 1.0,
        }
    }
}

/// 将 Rect 转换为 kurbo::Rect
pub fn rect_to_kurbo(rect: &Rect) -> KurboRect {
    KurboRect::new(
        rect.x as f64,
        rect.y as f64,
        (rect.x + rect.width) as f64,
        (rect.y + rect.height) as f64,
    )
}
