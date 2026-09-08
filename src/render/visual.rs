//! VisualElement — 纯数据渲染描述（基于 liecharts）
use crate::geometry::Color;
use crate::text::TextLayout;
use crate::view::paint::ImageFit;
pub use kurbo::{Affine, BezPath, Point as KPoint, Rect as KRect, Vec2};
use std::sync::Arc;

pub type TextLayoutRef = Arc<TextLayout>;

/// 2D 变换
#[derive(Debug, Clone, Copy, Default)]
pub struct Transform {
    pub translate: Vec2,
    pub rotate: f64,
    pub scale: Vec2,
}
impl Transform {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn translate(x: f64, y: f64) -> Self {
        Self {
            translate: Vec2::new(x, y),
            ..Default::default()
        }
    }
    pub fn to_affine(&self) -> Affine {
        Affine::translate(self.translate)
            * Affine::rotate(self.rotate)
            * Affine::scale_non_uniform(self.scale.x, self.scale.y)
    }
}

/// 填充+描边样式
#[derive(Debug, Clone, Default)]
pub struct FillStrokeStyle {
    pub fill: Option<Color>,
    pub stroke: Option<Stroke>,
}
impl FillStrokeStyle {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_fill(mut self, c: Color) -> Self {
        self.fill = Some(c);
        self
    }
    pub fn with_stroke(mut self, c: Color, w: f64) -> Self {
        self.stroke = Some(Stroke::new(c, w));
        self
    }
}

/// 描边
#[derive(Debug, Clone)]
pub struct Stroke {
    pub color: Color,
    pub width: f64,
}
impl Stroke {
    pub fn new(c: Color, w: f64) -> Self {
        Self { color: c, width: w }
    }
}
impl Default for Stroke {
    fn default() -> Self {
        Self {
            color: Color::BLACK,
            width: 1.0,
        }
    }
}

/// 带 z_index 的视觉元素
#[derive(Debug, Clone)]
pub struct LayeredElement {
    pub element: VisualElement,
    pub z_index: i32,
    pub element_id: Option<u64>,
}
impl LayeredElement {
    pub fn new(e: VisualElement, z: i32) -> Self {
        Self {
            element: e,
            z_index: z,
            element_id: None,
        }
    }
    pub fn with_id(mut self, id: u64) -> Self {
        self.element_id = Some(id);
        self
    }
    pub fn z_index(&self) -> i32 {
        self.z_index
    }
}

/// 视觉元素枚举（基于 liecharts，使用 kurbo 类型）
pub enum VisualElement {
    Rect {
        rect: KRect,
        style: FillStrokeStyle,
    },
    RoundedRect {
        rect: KRect,
        radius: f64,
        style: FillStrokeStyle,
    },
    /// 投影（PatternFly 的 box-shadow）：用高斯模糊圆角矩形绘制。
    ShadowRoundedRect {
        rect: KRect,
        radius: f64,
        std_dev: f64,
        color: Color,
    },
    Circle {
        center: KPoint,
        radius: f64,
        style: FillStrokeStyle,
    },
    Line {
        start: KPoint,
        end: KPoint,
        style: Stroke,
    },
    Path {
        path: BezPath,
        style: FillStrokeStyle,
    },
    TextRun {
        text: Arc<str>,
        position: KPoint,
        color: Color,
        font_size: f64,
        font_family: String,
        rotation: f64,
        max_width: Option<f64>,
        layout: Option<TextLayoutRef>,
    },
    Image {
        bounds: KRect,
        data: Arc<Vec<u8>>,
        width: u32,
        height: u32,
        opacity: Option<f32>,
        /// 内容缩放模式（Contain/Cover/Fill/None）。
        fit: ImageFit,
        /// 圆角半径（像素），0 表示不裁剪。
        border_radius: f32,
    },
    /// 共享像素表面：携带 surface 在窗口中的矩形，由 compositor 按脏区合屏。
    /// 不直接进入 vello 光栅化，而是由 compositor 从 surface buffer 拷贝脏区。
    SharedSurface {
        id: crate::render::surface::SurfaceId,
        bounds: KRect,
        width: u32,
        height: u32,
    },
    Group {
        children: Vec<LayeredElement>,
        transform: Option<Transform>,
        clip_rect: Option<KRect>,
    },
}

impl LayeredElement {
    /// 元素的内容签名（用于帧间比较：同签名 ⇒ 渲染结果相同）。
    ///
    /// 覆盖 z 序、几何、颜色、文本、图像数据指针等全部影响像素的字段。
    /// 任何影响输出的字段都必须进签名，否则脏区判定会漏更新。
    pub fn signature(&self) -> u64 {
        use std::hash::Hasher as _;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.hash_signature(&mut h);
        h.finish()
    }

    fn hash_signature<H: std::hash::Hasher>(&self, h: &mut H) {
        use std::hash::Hash;
        self.z_index.hash(h);
        self.element.hash_signature(h);
    }
}

/// 找一个可选参数的稳定标识（用于把 `Option<T>` 混入签名）。
fn hash_opt<H: std::hash::Hasher, T: std::hash::Hash>(h: &mut H, v: &Option<T>) {
    use std::hash::Hash;
    match v {
        Some(t) => {
            1u8.hash(h);
            t.hash(h);
        }
        None => 0u8.hash(h),
    }
}

/// 把 kurbo 矩形按位混入签名（避免浮点 `Hash` 缺失与 NaN 语义差异）。
fn hash_rect<H: std::hash::Hasher>(h: &mut H, r: &KRect) {
    use std::hash::Hash;
    r.x0.to_bits().hash(h);
    r.y0.to_bits().hash(h);
    r.x1.to_bits().hash(h);
    r.y1.to_bits().hash(h);
}

/// 颜色不实现 `Hash`，这里按通道混入。
fn hash_color<H: std::hash::Hasher>(h: &mut H, c: &Color) {
    use std::hash::Hash;
    (c.r, c.g, c.b, c.a).hash(h);
}

impl FillStrokeStyle {
    fn hash_signature<H: std::hash::Hasher>(&self, h: &mut H) {
        use std::hash::Hash;
        match &self.fill {
            Some(c) => {
                1u8.hash(h);
                hash_color(h, c);
            }
            None => 0u8.hash(h),
        }
        match &self.stroke {
            Some(s) => {
                1u8.hash(h);
                hash_color(h, &s.color);
                s.width.to_bits().hash(h);
            }
            None => 0u8.hash(h),
        }
    }
}

impl VisualElement {
    fn hash_signature<H: std::hash::Hasher>(&self, h: &mut H) {
        use std::hash::Hash;
        // 变体序号：不同图元即使字段同名也不相等。
        let tag: u8 = match self {
            Self::Rect { .. } => 0,
            Self::RoundedRect { .. } => 1,
            Self::ShadowRoundedRect { .. } => 2,
            Self::Circle { .. } => 3,
            Self::Line { .. } => 4,
            Self::Path { .. } => 5,
            Self::TextRun { .. } => 6,
            Self::Image { .. } => 7,
            Self::SharedSurface { .. } => 8,
            Self::Group { .. } => 9,
        };
        tag.hash(h);
        match self {
            Self::Rect { rect, style } => {
                hash_rect(h, rect);
                style.hash_signature(h);
            }
            Self::RoundedRect {
                rect,
                radius,
                style,
            } => {
                hash_rect(h, rect);
                radius.to_bits().hash(h);
                style.hash_signature(h);
            }
            Self::ShadowRoundedRect {
                rect,
                radius,
                std_dev,
                color,
            } => {
                hash_rect(h, rect);
                radius.to_bits().hash(h);
                std_dev.to_bits().hash(h);
                hash_color(h, color);
            }
            Self::Circle {
                center,
                radius,
                style,
            } => {
                center.x.to_bits().hash(h);
                center.y.to_bits().hash(h);
                radius.to_bits().hash(h);
                style.hash_signature(h);
            }
            Self::Line { start, end, style } => {
                start.x.to_bits().hash(h);
                start.y.to_bits().hash(h);
                end.x.to_bits().hash(h);
                end.y.to_bits().hash(h);
                hash_color(h, &style.color);
                style.width.to_bits().hash(h);
            }
            Self::Path { path, style } => {
                use kurbo::ParamCurve as _;
                // 路径元素少且结构复杂：用段数 + 各段起点近似（足够区分常见变化）。
                (path.segments().count() as u64).hash(h);
                for seg in path.segments() {
                    let p = seg.start();
                    p.x.to_bits().hash(h);
                    p.y.to_bits().hash(h);
                }
                style.hash_signature(h);
            }
            Self::TextRun {
                text,
                position,
                color,
                font_size,
                font_family,
                rotation,
                max_width,
                layout,
            } => {
                text.hash(h);
                position.x.to_bits().hash(h);
                position.y.to_bits().hash(h);
                hash_color(h, color);
                font_size.to_bits().hash(h);
                font_family.hash(h);
                rotation.to_bits().hash(h);
                hash_opt(h, &max_width.map(|v| v.to_bits()));
                // 排版结果：以指针标识（内容变化必然重新排版）。
                (layout.as_ref().map_or(0usize, |l| Arc::as_ptr(l) as usize)).hash(h);
            }
            Self::Image {
                bounds,
                data,
                width,
                height,
                opacity,
                fit,
                border_radius,
            } => {
                hash_rect(h, bounds);
                (Arc::as_ptr(data) as usize).hash(h);
                width.hash(h);
                height.hash(h);
                hash_opt(h, &opacity.map(|v| v.to_bits()));
                let fit_tag: u8 = match fit {
                    ImageFit::None => 0,
                    ImageFit::Fill => 1,
                    ImageFit::Contain => 2,
                    ImageFit::Cover => 3,
                };
                fit_tag.hash(h);
                border_radius.to_bits().hash(h);
            }
            Self::SharedSurface { id, bounds, .. } => {
                id.0.hash(h);
                hash_rect(h, bounds);
            }
            Self::Group {
                children,
                transform,
                clip_rect,
            } => {
                (children.len() as u64).hash(h);
                for c in children {
                    c.hash_signature(h);
                }
                hash_opt(
                    h,
                    &transform.map(|t| {
                        (
                            t.translate.x.to_bits(),
                            t.translate.y.to_bits(),
                            t.rotate.to_bits(),
                            t.scale.x.to_bits(),
                            t.scale.y.to_bits(),
                        )
                    }),
                );
                if let Some(c) = clip_rect {
                    hash_rect(h, c);
                } else {
                    0u8.hash(h);
                }
            }
        }
    }

    /// 获取元素的近似边界矩形（用于视口剔除）。
    /// Group 返回 None（由内部子元素单独剔除）。
    pub fn bounding_rect(&self) -> Option<KRect> {
        match self {
            Self::Rect { rect, .. } => Some(*rect),
            Self::RoundedRect { rect, .. } => Some(*rect),
            Self::ShadowRoundedRect { rect, .. } => Some(*rect),
            Self::Circle { center, radius, .. } => Some(KRect::new(
                center.x - radius,
                center.y - radius,
                center.x + radius,
                center.y + radius,
            )),
            Self::Line { start, end, .. } => Some(KRect::new(
                start.x.min(end.x),
                start.y.min(end.y),
                start.x.max(end.x),
                start.y.max(end.y),
            )),
            Self::Path { .. } => None, // 无法简单计算
            Self::TextRun {
                position,
                font_size,
                ..
            } => {
                // 粗略估算边界用于视口剔除，不需要精确宽度
                Some(KRect::new(
                    position.x,
                    position.y,
                    position.x + 2000.0, // 横向不截断，由 clip 而不是 bounding 决定
                    position.y + font_size,
                ))
            }
            Self::Image { bounds, .. } => Some(*bounds),
            Self::SharedSurface { bounds, .. } => Some(*bounds),
            Self::Group { .. } => None,
        }
    }
}
impl std::fmt::Debug for VisualElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rect { rect, style } => f
                .debug_struct("Rect")
                .field("rect", rect)
                .field("style", style)
                .finish(),
            Self::RoundedRect { rect, radius, .. } => f
                .debug_struct("RoundedRect")
                .field("rect", rect)
                .field("radius", radius)
                .finish(),
            Self::ShadowRoundedRect {
                rect,
                radius,
                std_dev,
                ..
            } => f
                .debug_struct("ShadowRoundedRect")
                .field("rect", rect)
                .field("radius", radius)
                .field("std_dev", std_dev)
                .finish(),
            Self::TextRun {
                text,
                color,
                font_size,
                ..
            } => f
                .debug_struct("TextRun")
                .field("text", text)
                .field("color", color)
                .field("font_size", font_size)
                .finish(),
            Self::Group {
                children,
                transform,
                clip_rect: _,
            } => f
                .debug_struct("Group")
                .field("children", &children.len())
                .field("transform", transform)
                .finish(),
            _ => write!(f, "{:?}", std::mem::discriminant(self)),
        }
    }
}

impl Clone for VisualElement {
    fn clone(&self) -> Self {
        match self {
            Self::Rect { rect, style } => Self::Rect {
                rect: *rect,
                style: style.clone(),
            },
            Self::RoundedRect {
                rect,
                radius,
                style,
                ..
            } => Self::RoundedRect {
                rect: *rect,
                radius: *radius,
                style: style.clone(),
            },
            Self::ShadowRoundedRect {
                rect,
                radius,
                std_dev,
                color,
            } => Self::ShadowRoundedRect {
                rect: *rect,
                radius: *radius,
                std_dev: *std_dev,
                color: *color,
            },
            Self::Circle {
                center,
                radius,
                style,
            } => Self::Circle {
                center: *center,
                radius: *radius,
                style: style.clone(),
            },
            Self::Line { start, end, style } => Self::Line {
                start: *start,
                end: *end,
                style: style.clone(),
            },
            Self::Path { path, style } => Self::Path {
                path: path.clone(),
                style: style.clone(),
            },
            Self::TextRun {
                text,
                position,
                color,
                font_size,
                font_family,
                rotation,
                max_width,
                layout,
            } => Self::TextRun {
                text: text.clone(),
                position: *position,
                color: *color,
                font_size: *font_size,
                font_family: font_family.clone(),
                rotation: *rotation,
                max_width: *max_width,
                layout: layout.clone(),
            },
            Self::Image {
                bounds,
                data,
                width,
                height,
                opacity,
                fit,
                border_radius,
            } => Self::Image {
                bounds: *bounds,
                data: data.clone(),
                width: *width,
                height: *height,
                opacity: *opacity,
                fit: *fit,
                border_radius: *border_radius,
            },
            Self::SharedSurface {
                id,
                bounds,
                width,
                height,
            } => Self::SharedSurface {
                id: *id,
                bounds: *bounds,
                width: *width,
                height: *height,
            },
            Self::Group {
                children,
                transform,
                clip_rect,
            } => Self::Group {
                children: children.clone(),
                transform: *transform,
                clip_rect: *clip_rect,
            },
        }
    }
}
