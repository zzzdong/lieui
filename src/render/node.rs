// src/render/node.rs

use crate::geometry::{Color, Rect};
use crate::text::TextLayout;

/// 盒阴影效果
#[derive(Debug, Clone, PartialEq)]
pub struct BoxShadow {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur_radius: f32,
    pub spread_radius: f32,
    pub color: Color,
}

impl BoxShadow {
    pub fn new(color: impl Into<Color>) -> Self {
        Self {
            offset_x: 0.0,
            offset_y: 2.0,
            blur_radius: 4.0,
            spread_radius: 0.0,
            color: color.into(),
        }
    }

    pub fn offset(mut self, x: f32, y: f32) -> Self {
        self.offset_x = x;
        self.offset_y = y;
        self
    }

    pub fn blur(mut self, radius: f32) -> Self {
        self.blur_radius = radius;
        self
    }

    pub fn spread(mut self, radius: f32) -> Self {
        self.spread_radius = radius;
        self
    }
}

/// 渲染节点类型
pub enum RenderNode {
    /// 视图容器
    View {
        bounds: Rect,
        background: Option<Color>,
        border_color: Option<Color>,
        border_width: Option<f32>,
        border_radius: Option<f32>,
        opacity: Option<f32>,
        box_shadow: Option<BoxShadow>,
        children: Vec<RenderNode>,
    },
    /// 通用容器
    Div {
        bounds: Rect,
        background: Option<Color>,
        border_color: Option<Color>,
        border_width: Option<f32>,
        border_radius: Option<f32>,
        opacity: Option<f32>,
        box_shadow: Option<BoxShadow>,
        children: Vec<RenderNode>,
    },
    /// 文本节点
    Text { bounds: Rect, layout: TextLayout },
    /// 图片节点
    Image {
        bounds: Rect,
        width: u32,
        height: u32,
        data: Vec<u8>,
        opacity: Option<f32>,
    },
    /// 画布节点
    Canvas { bounds: Rect, draw: CanvasCallback },
}

pub type CanvasCallback = Box<dyn Fn(&mut vello_cpu::RenderContext, Rect)>;

impl RenderNode {
    /// 创建 View 节点
    pub fn view(bounds: Rect) -> Self {
        Self::View {
            bounds,
            background: None,
            border_color: None,
            border_width: None,
            border_radius: None,
            opacity: None,
            box_shadow: None,
            children: Vec::new(),
        }
    }

    /// 创建 Div 节点
    pub fn div(bounds: Rect) -> Self {
        Self::Div {
            bounds,
            background: None,
            border_color: None,
            border_width: None,
            border_radius: None,
            opacity: None,
            box_shadow: None,
            children: Vec::new(),
        }
    }

    /// 创建 Text 节点
    pub fn text(bounds: Rect, layout: TextLayout) -> Self {
        Self::Text { bounds, layout }
    }

    /// 创建 Image 节点
    pub fn image(bounds: Rect, width: u32, height: u32, data: Vec<u8>) -> Self {
        Self::Image {
            bounds,
            width,
            height,
            data,
            opacity: None,
        }
    }

    /// 创建 Canvas 节点
    pub fn canvas<F>(bounds: Rect, draw: F) -> Self
    where
        F: Fn(&mut vello_cpu::RenderContext, Rect) + 'static,
    {
        Self::Canvas {
            bounds,
            draw: Box::new(draw),
        }
    }

    // ===== 通用属性设置方法 =====

    /// 设置背景色
    pub fn background(mut self, color: impl Into<Color>) -> Self {
        match &mut self {
            Self::View { background, .. } | Self::Div { background, .. } => {
                *background = Some(color.into())
            }
            _ => {}
        }
        self
    }

    /// 设置边框颜色
    pub fn border_color(mut self, color: impl Into<Color>) -> Self {
        match &mut self {
            Self::View { border_color, .. } | Self::Div { border_color, .. } => {
                *border_color = Some(color.into())
            }
            _ => {}
        }
        self
    }

    /// 设置边框宽度
    pub fn border_width(mut self, width: f32) -> Self {
        match &mut self {
            Self::View { border_width, .. } | Self::Div { border_width, .. } => {
                *border_width = Some(width)
            }
            _ => {}
        }
        self
    }

    /// 设置圆角半径
    pub fn border_radius(mut self, radius: f32) -> Self {
        match &mut self {
            Self::View { border_radius, .. } | Self::Div { border_radius, .. } => {
                *border_radius = Some(radius)
            }
            _ => {}
        }
        self
    }

    /// 设置透明度
    pub fn opacity(mut self, opacity: f32) -> Self {
        match &mut self {
            Self::View { opacity: op, .. }
            | Self::Div { opacity: op, .. }
            | Self::Image { opacity: op, .. } => *op = Some(opacity),
            _ => {}
        }
        self
    }

    /// 设置盒阴影
    pub fn box_shadow(mut self, shadow: BoxShadow) -> Self {
        match &mut self {
            Self::View { box_shadow: s, .. } | Self::Div { box_shadow: s, .. } => *s = Some(shadow),
            _ => {}
        }
        self
    }

    /// 添加子节点（仅 View 和 Div 节点有效）
    pub fn add_child(mut self, child: RenderNode) -> Self {
        match &mut self {
            Self::View { children, .. } | Self::Div { children, .. } => children.push(child),
            _ => {}
        }
        self
    }

    /// 获取边界
    pub fn bounds(&self) -> Rect {
        match self {
            Self::View { bounds, .. }
            | Self::Div { bounds, .. }
            | Self::Text { bounds, .. }
            | Self::Image { bounds, .. }
            | Self::Canvas { bounds, .. } => *bounds,
        }
    }

    /// 获取子节点（仅 View 和 Div 节点有效）
    pub fn children(&self) -> &[RenderNode] {
        match self {
            Self::View { children, .. } | Self::Div { children, .. } => children,
            _ => &[],
        }
    }

    /// 获取子节点的可变引用（仅 View 和 Div 节点有效）
    ///
    /// 如果节点不是容器类型，返回 None
    pub fn try_children_mut(&mut self) -> Option<&mut Vec<RenderNode>> {
        match self {
            Self::View { children, .. } | Self::Div { children, .. } => Some(children),
            _ => None,
        }
    }

    /// 添加子节点（仅 View 和 Div 节点有效）
    pub fn push_child(&mut self, child: RenderNode) {
        if let Some(children) = self.try_children_mut() {
            children.push(child);
        }
    }

    /// 转换为 XML 字符串（用于调试）
    pub fn to_xml(&self, indent: usize) -> String {
        let spaces = "  ".repeat(indent);
        match self {
            Self::View {
                bounds,
                background,
                border_radius,
                children,
                ..
            } => {
                let mut xml = format!(
                    "{}<View x=\"{}\" y=\"{}\" w=\"{}\" h=\"{}\"",
                    spaces, bounds.x, bounds.y, bounds.width, bounds.height
                );
                if let Some(bg) = background {
                    xml.push_str(&format!(" bg=\"{}\"", bg.to_hex()));
                }
                if let Some(radius) = border_radius {
                    xml.push_str(&format!(" borderRadius=\"{}\"", radius));
                }
                if children.is_empty() {
                    xml.push_str(" />");
                } else {
                    xml.push_str(">");
                    for child in children {
                        xml.push('\n');
                        xml.push_str(&child.to_xml(indent + 1));
                    }
                    xml.push_str(&format!("\n{}</View>", spaces));
                }
                xml
            }
            Self::Div {
                bounds,
                background,
                border_radius,
                children,
                ..
            } => {
                let mut xml = format!(
                    "{}<Div x=\"{}\" y=\"{}\" w=\"{}\" h=\"{}\"",
                    spaces, bounds.x, bounds.y, bounds.width, bounds.height
                );
                if let Some(bg) = background {
                    xml.push_str(&format!(" bg=\"{}\"", bg.to_hex()));
                }
                if let Some(radius) = border_radius {
                    xml.push_str(&format!(" borderRadius=\"{}\"", radius));
                }
                if children.is_empty() {
                    xml.push_str(" />");
                } else {
                    xml.push_str(">");
                    for child in children {
                        xml.push('\n');
                        xml.push_str(&child.to_xml(indent + 1));
                    }
                    xml.push_str(&format!("\n{}</Div>", spaces));
                }
                xml
            }
            Self::Text { bounds, .. } => {
                // 文本内容不再从 layout 获取，因为 layout.data() 是私有的
                format!(
                    "{}<Text x=\"{}\" y=\"{}\" w=\"{}\" h=\"{}\"/>",
                    spaces, bounds.x, bounds.y, bounds.width, bounds.height
                )
            }
            Self::Image {
                bounds,
                width,
                height,
                ..
            } => {
                format!(
                    "{}<Image x=\"{}\" y=\"{}\" w=\"{}\" h=\"{}\" srcWidth=\"{}\" srcHeight=\"{}\" />",
                    spaces, bounds.x, bounds.y, bounds.width, bounds.height, width, height
                )
            }
            Self::Canvas { bounds, .. } => {
                format!(
                    "{}<Canvas x=\"{}\" y=\"{}\" w=\"{}\" h=\"{}\" />",
                    spaces, bounds.x, bounds.y, bounds.width, bounds.height
                )
            }
        }
    }
}

// 手动实现 Debug trait
impl std::fmt::Debug for RenderNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::View {
                bounds,
                background,
                children,
                ..
            } => f
                .debug_struct("View")
                .field("bounds", bounds)
                .field("background", background)
                .field("children", children)
                .finish(),
            Self::Div {
                bounds,
                background,
                children,
                ..
            } => f
                .debug_struct("Div")
                .field("bounds", bounds)
                .field("background", background)
                .field("children", children)
                .finish(),
            Self::Text { bounds, .. } => f.debug_struct("Text").field("bounds", bounds).finish(),
            Self::Image {
                bounds,
                width,
                height,
                ..
            } => f
                .debug_struct("Image")
                .field("bounds", bounds)
                .field("width", width)
                .field("height", height)
                .finish(),
            Self::Canvas { bounds, .. } => {
                f.debug_struct("Canvas").field("bounds", bounds).finish()
            }
        }
    }
}

// 为 Canvas 实现 Clone（draw 回调无法克隆，所以设为 None）
impl Clone for RenderNode {
    fn clone(&self) -> Self {
        match self {
            Self::View {
                bounds,
                background,
                border_color,
                border_width,
                border_radius,
                opacity,
                box_shadow,
                children,
            } => Self::View {
                bounds: *bounds,
                background: background.clone(),
                border_color: border_color.clone(),
                border_width: *border_width,
                border_radius: *border_radius,
                opacity: *opacity,
                box_shadow: box_shadow.clone(),
                children: children.clone(),
            },
            Self::Div {
                bounds,
                background,
                border_color,
                border_width,
                border_radius,
                opacity,
                box_shadow,
                children,
            } => Self::Div {
                bounds: *bounds,
                background: background.clone(),
                border_color: border_color.clone(),
                border_width: *border_width,
                border_radius: *border_radius,
                opacity: *opacity,
                box_shadow: box_shadow.clone(),
                children: children.clone(),
            },
            Self::Text { bounds, layout } => Self::Text {
                bounds: *bounds,
                layout: layout.clone(),
            },
            Self::Image {
                bounds,
                width,
                height,
                data,
                opacity,
            } => Self::Image {
                bounds: *bounds,
                width: *width,
                height: *height,
                data: data.clone(),
                opacity: *opacity,
            },
            Self::Canvas { bounds, .. } => {
                // Canvas 无法克隆回调函数
                Self::Canvas {
                    bounds: *bounds,
                    draw: Box::new(|_, _| {}),
                }
            }
        }
    }
}
