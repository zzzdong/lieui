//! ViewNode — Builder 侧纯数据 UI 描述（原语枚举）
//!
//! 基本原语参照 HTML：
//! - Text：文本节点
//! - Image：图片节点
//! - Div：通用容器，可工作在 Block 模式（类似 `div`）或 Flex 模式（类似 `display: flex`）
//! - Canvas：自定义绘制预留原语（当前不渲染任何内容）
//!
//! 每个原语都可以附带 `listener: Option<ClickCallbackRef>` 来响应点击事件。
//! 所有高级 widget（Button、Checkbox、Container、Column、Row、Divider）
//! 都在 `primitives.rs` 中通过这些原语组合而成。

use crate::core::{ElementId, ElementState};
use crate::geometry::Color;
use crate::layout::box_model::{BoxStyle, ComputedLayout, IntrinsicSize, LayoutConstraint};
use crate::layout::flex::FlexStyle;
use crate::layout::measurable::{EmptyMeasure, FixedMeasure, Measurable, TextMeasure};
use crate::render::visual::{FillStrokeStyle, KRect, LayeredElement as LE, VisualElement};
use crate::text::create_text_layout;

/// Listener 上注册的回调引用
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickCallbackRef {
    Simple(u64),
    WithCtx(u64),
}

impl ClickCallbackRef {
    pub fn id(self) -> u64 {
        match self {
            ClickCallbackRef::Simple(id) | ClickCallbackRef::WithCtx(id) => id,
        }
    }
}

/// 节点类型标记
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Text,
    Image,
    Div,
    Canvas,
}

/// Div 的布局显示模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    /// 块级容器：仅使用 BoxStyle 的盒模型属性
    Block,
    /// Flex 容器：使用 FlexStyle 的一维布局属性
    Flex,
}

/// 布局样式 — Runtime 从 ViewNode 提取后生成 LayoutNode
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutStyle {
    pub node_type: NodeType,
    pub display: DisplayMode,
    pub flex: FlexStyle,
    pub box_style: BoxStyle,
}

impl Default for LayoutStyle {
    fn default() -> Self {
        Self {
            node_type: NodeType::Div,
            display: DisplayMode::Block,
            flex: FlexStyle::default(),
            box_style: BoxStyle::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ViewNode {
    Text {
        content: String,
        font_size: f64,
        color: Color,
        key: Option<String>,
        listener: Option<ClickCallbackRef>,
    },
    Image {
        data: std::sync::Arc<Vec<u8>>,
        w: u32,
        h: u32,
        key: Option<String>,
        listener: Option<ClickCallbackRef>,
    },
    Div {
        style: BoxStyle,
        flex: FlexStyle,
        display: DisplayMode,
        key: Option<String>,
        children: Vec<ViewNode>,
        listener: Option<ClickCallbackRef>,
    },
    Canvas {
        key: Option<String>,
        listener: Option<ClickCallbackRef>,
    },
}

impl ViewNode {
    /// 返回可读的类型名称字符串（调试用）
    pub fn node_type_name(&self) -> &'static str {
        self.type_name()
    }

    pub fn node_type(&self) -> NodeType {
        match self {
            ViewNode::Text { .. } => NodeType::Text,
            ViewNode::Image { .. } => NodeType::Image,
            ViewNode::Div { .. } => NodeType::Div,
            ViewNode::Canvas { .. } => NodeType::Canvas,
        }
    }

    pub fn display_mode(&self) -> Option<DisplayMode> {
        match self {
            ViewNode::Div { display, .. } => Some(*display),
            _ => None,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            ViewNode::Text { .. } => "text",
            ViewNode::Image { .. } => "image",
            ViewNode::Div { display, .. } => match display {
                DisplayMode::Block => "div",
                DisplayMode::Flex => "flex",
            },
            ViewNode::Canvas { .. } => "canvas",
        }
    }

    pub fn key(&self) -> Option<&str> {
        match self {
            ViewNode::Text { key, .. }
            | ViewNode::Image { key, .. }
            | ViewNode::Div { key, .. }
            | ViewNode::Canvas { key, .. } => key.as_deref(),
        }
    }

    pub fn set_key(&mut self, k: String) {
        match self {
            ViewNode::Text { key, .. }
            | ViewNode::Image { key, .. }
            | ViewNode::Div { key, .. }
            | ViewNode::Canvas { key, .. } => *key = Some(k),
        }
    }

    pub fn set_listener(&mut self, l: Option<ClickCallbackRef>) {
        match self {
            ViewNode::Text { listener, .. }
            | ViewNode::Image { listener, .. }
            | ViewNode::Div { listener, .. }
            | ViewNode::Canvas { listener, .. } => *listener = l,
        }
    }

    pub fn with_listener(mut self, l: Option<ClickCallbackRef>) -> Self {
        self.set_listener(l);
        self
    }

    pub fn children(&self) -> &[ViewNode] {
        match self {
            ViewNode::Div { children, .. } => children,
            _ => &[],
        }
    }

    pub fn layout_style(&self) -> LayoutStyle {
        match self {
            ViewNode::Text { .. } => LayoutStyle {
                node_type: NodeType::Text,
                ..LayoutStyle::default()
            },
            ViewNode::Image { .. } => LayoutStyle {
                node_type: NodeType::Image,
                ..LayoutStyle::default()
            },
            ViewNode::Div {
                style,
                flex,
                display,
                ..
            } => LayoutStyle {
                node_type: NodeType::Div,
                display: *display,
                box_style: style.clone(),
                flex: flex.clone(),
            },
            ViewNode::Canvas { .. } => LayoutStyle {
                node_type: NodeType::Canvas,
                ..LayoutStyle::default()
            },
        }
    }

    /// 在约束下测量自身固有尺寸。
    pub fn measure(&self, constraint: &LayoutConstraint) -> IntrinsicSize {
        match self {
            ViewNode::Text {
                content, font_size, ..
            } => TextMeasure::new(content.clone(), *font_size).measure(constraint),
            ViewNode::Image { w, h, .. } => {
                FixedMeasure::new(IntrinsicSize::new(*w as f32, *h as f32)).measure(constraint)
            }
            ViewNode::Div { style, .. } => {
                let w = style.fixed_width.unwrap_or(0.0);
                let h = style.fixed_height.unwrap_or(0.0);
                if style.fixed_width.is_some() || style.fixed_height.is_some() {
                    FixedMeasure::new(IntrinsicSize::new(w, h)).measure(constraint)
                } else {
                    EmptyMeasure.measure(constraint)
                }
            }
            ViewNode::Canvas { .. } => EmptyMeasure.measure(constraint),
        }
    }

    /// 比较"配置"部分是否相等（排除 children，因为孩子由树结构管理）
    pub fn config_eq(&self, other: &Self) -> bool {
        use ViewNode::*;
        match (self, other) {
            (
                Text {
                    content: a,
                    font_size: b,
                    color: c,
                    listener: l1,
                    ..
                },
                Text {
                    content: x,
                    font_size: y,
                    color: z,
                    listener: l2,
                    ..
                },
            ) => a == x && (b - y).abs() < 0.001 && c == z && l1 == l2,
            (
                Image {
                    data: a,
                    w: b,
                    h: c,
                    listener: l1,
                    ..
                },
                Image {
                    data: x,
                    w: y,
                    h: z,
                    listener: l2,
                    ..
                },
            ) => a == x && b == y && c == z && l1 == l2,
            (
                Div {
                    style: s1,
                    flex: f1,
                    display: d1,
                    listener: l1,
                    ..
                },
                Div {
                    style: s2,
                    flex: f2,
                    display: d2,
                    listener: l2,
                    ..
                },
            ) => s1 == s2 && f1 == f2 && d1 == d2 && l1 == l2,
            (Canvas { listener: l1, .. }, Canvas { listener: l2, .. }) => l1 == l2,
            _ => false,
        }
    }

    pub fn is_container(&self) -> bool {
        matches!(self, ViewNode::Div { .. })
    }

    pub fn listener(&self) -> Option<ClickCallbackRef> {
        match self {
            ViewNode::Text { listener, .. }
            | ViewNode::Image { listener, .. }
            | ViewNode::Div { listener, .. }
            | ViewNode::Canvas { listener, .. } => *listener,
        }
    }

    pub fn on_click(&self) -> Option<ClickCallbackRef> {
        self.listener()
    }

    pub fn on_click_id(&self) -> Option<u64> {
        self.listener().map(|c| c.id())
    }

    pub fn render(
        &self,
        layout: &ComputedLayout,
        state: ElementState,
        elements: &mut Vec<LE>,
        z_index: i32,
        element_id: ElementId,
    ) {
        let r = layout.rect();
        match self {
            ViewNode::Text {
                content,
                font_size,
                color,
                ..
            } => {
                let lay = create_text_layout(content, *font_size, *color, None);
                elements.push(
                    LE::new(
                        VisualElement::TextRun {
                            text: std::sync::Arc::from(content.as_str()),
                            position: kurbo::Point::new(r.x as f64, r.y as f64),
                            color: *color,
                            font_size: *font_size,
                            font_family: "sans-serif".to_string(),
                            rotation: 0.0,
                            max_width: None,
                            layout: Some(Box::new(lay)),
                        },
                        z_index,
                    )
                    .with_id(element_id.as_ffi()),
                );
            }
            ViewNode::Image { data, w, h, .. } => {
                elements.push(
                    LE::new(
                        VisualElement::Image {
                            bounds: KRect::new(
                                r.x as f64,
                                r.y as f64,
                                (r.x + r.width) as f64,
                                (r.y + r.height) as f64,
                            ),
                            data: std::sync::Arc::clone(data),
                            width: *w,
                            height: *h,
                            opacity: None,
                        },
                        z_index,
                    )
                    .with_id(element_id.as_ffi()),
                );
            }
            ViewNode::Div { style, .. } => {
                let bg = if state.pressed && style.pressed_background.is_some() {
                    style.pressed_background
                } else if state.hovered && style.hover_background.is_some() {
                    style.hover_background
                } else {
                    style.background_color
                };
                if let Some(bg) = bg {
                    let k = KRect::new(
                        r.x as f64,
                        r.y as f64,
                        (r.x + r.width) as f64,
                        (r.y + r.height) as f64,
                    );
                    elements.push(
                        LE::new(
                            VisualElement::RoundedRect {
                                rect: k,
                                radius: style.border_radius as f64,
                                style: {
                                    let mut fs = FillStrokeStyle::new().with_fill(bg);
                                    if let Some(bc) = style.border_color {
                                        if style.border_width > 0.0 {
                                            fs = fs.with_stroke(bc, style.border_width as f64);
                                        }
                                    }
                                    fs
                                },
                            },
                            z_index,
                        )
                        .with_id(element_id.as_ffi()),
                    );
                }
            }
            ViewNode::Canvas { .. } => {
                // Canvas 当前为预留原语，不渲染任何内容。
                // 未来可在此触发自定义绘制回调或生成 VisualElement::Custom。
            }
        }
    }
}
