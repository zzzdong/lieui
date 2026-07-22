//! ViewNode — Builder 侧纯数据 UI 描述（原语枚举）
//!
//! 只有五种基本原语：
//! - Text / Image：叶子节点
//! - Box：装饰性容器（padding + background + 固定或展开尺寸）
//! - Flex：一维布局容器（Row / Column）
//! - Listener：事件监听包装器，透传给唯一子节点
//!
//! 所有高级 widget（Button、Checkbox、Container、Column、Row、Divider）
//! 都在 `primitives.rs` 中通过这五种原语组合而成。

use crate::core::{ElementId, ElementState};
use crate::geometry::Color;
use crate::layout::box_model::{BoxStyle, ComputedLayout, IntrinsicSize, LayoutConstraint};
use crate::layout::flex::{AlignItems, FlexDirection, FlexStyle, JustifyContent};
use crate::layout::measurable::{EmptyMeasure, FixedMeasure, Measurable, TextMeasure};
use crate::render::visual::{FillStrokeStyle, KRect, LayeredElement as LE, VisualElement};
use crate::text::create_text_layout;

/// 节点类型标记
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Text,
    Image,
    Box,
    Flex,
    Listener,
}

/// 布局样式 — Runtime 从 ViewNode 提取后生成 LayoutNode
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutStyle {
    pub node_type: NodeType,
    pub flex: FlexStyle,
    pub box_style: BoxStyle,
}

impl Default for LayoutStyle {
    fn default() -> Self {
        Self {
            node_type: NodeType::Box,
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
    },
    Image {
        data: Vec<u8>,
        w: u32,
        h: u32,
        key: Option<String>,
    },
    Box {
        style: BoxStyle,
        key: Option<String>,
        children: Vec<ViewNode>,
    },
    Flex {
        direction: FlexDirection,
        justify: JustifyContent,
        align: AlignItems,
        spacing: f32,
        expand: bool,
        key: Option<String>,
        children: Vec<ViewNode>,
    },
    Listener {
        on_click: Option<u64>,
        key: Option<String>,
        child: Box<ViewNode>,
    },
}

impl ViewNode {
    pub fn type_name(&self) -> &'static str {
        match self {
            ViewNode::Text { .. } => "text",
            ViewNode::Image { .. } => "image",
            ViewNode::Box { .. } => "box",
            ViewNode::Flex { .. } => "flex",
            ViewNode::Listener { .. } => "listener",
        }
    }

    pub fn key(&self) -> Option<&str> {
        match self {
            ViewNode::Text { key, .. }
            | ViewNode::Image { key, .. }
            | ViewNode::Box { key, .. }
            | ViewNode::Flex { key, .. }
            | ViewNode::Listener { key, .. } => key.as_deref(),
        }
    }

    pub fn set_key(&mut self, k: String) {
        match self {
            ViewNode::Text { key, .. }
            | ViewNode::Image { key, .. }
            | ViewNode::Box { key, .. }
            | ViewNode::Flex { key, .. }
            | ViewNode::Listener { key, .. } => *key = Some(k),
        }
    }

    pub fn children(&self) -> &[ViewNode] {
        match self {
            ViewNode::Box { children, .. } | ViewNode::Flex { children, .. } => children,
            ViewNode::Listener { child, .. } => std::slice::from_ref(child),
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
            ViewNode::Box { style, .. } => LayoutStyle {
                node_type: NodeType::Box,
                box_style: style.clone(),
                ..LayoutStyle::default()
            },
            ViewNode::Flex {
                direction,
                justify,
                align,
                spacing,
                expand,
                ..
            } => LayoutStyle {
                node_type: NodeType::Flex,
                flex: FlexStyle {
                    direction: *direction,
                    justify: *justify,
                    align: *align,
                    spacing: *spacing,
                    expand: *expand,
                },
                ..LayoutStyle::default()
            },
            ViewNode::Listener { child, .. } => {
                // Listener 是透明包装，布局样式完全由子节点决定
                let mut style = child.layout_style();
                style.node_type = NodeType::Listener;
                style
            }
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
            ViewNode::Box { style, .. } => {
                if let (Some(fw), Some(fh)) = (style.fixed_width, style.fixed_height) {
                    FixedMeasure::new(IntrinsicSize::new(fw, fh)).measure(constraint)
                } else if let Some(fw) = style.fixed_width {
                    let h = style.fixed_height.unwrap_or(0.0);
                    FixedMeasure::new(IntrinsicSize::new(fw, h)).measure(constraint)
                } else if let Some(fh) = style.fixed_height {
                    let w = style.fixed_width.unwrap_or(0.0);
                    FixedMeasure::new(IntrinsicSize::new(w, fh)).measure(constraint)
                } else {
                    EmptyMeasure.measure(constraint)
                }
            }
            ViewNode::Flex { .. } => EmptyMeasure.measure(constraint),
            ViewNode::Listener { child, .. } => child.measure(constraint),
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
                    ..
                },
                Text {
                    content: x,
                    font_size: y,
                    color: z,
                    ..
                },
            ) => a == x && (b - y).abs() < 0.001 && c == z,
            (
                Image {
                    data: a,
                    w: b,
                    h: c,
                    ..
                },
                Image {
                    data: x,
                    w: y,
                    h: z,
                    ..
                },
            ) => a == x && b == y && c == z,
            (Box { style: a, .. }, Box { style: b, .. }) => a == b,
            (
                Flex {
                    direction: a,
                    justify: b,
                    align: c,
                    spacing: d,
                    expand: e,
                    ..
                },
                Flex {
                    direction: f,
                    justify: g,
                    align: h,
                    spacing: i,
                    expand: j,
                    ..
                },
            ) => a == f && b == g && c == h && (d - i).abs() < 0.001 && e == j,
            (Listener { on_click: a, .. }, Listener { on_click: b, .. }) => a == b,
            _ => false,
        }
    }

    pub fn is_container(&self) -> bool {
        matches!(
            self,
            ViewNode::Box { .. } | ViewNode::Flex { .. } | ViewNode::Listener { .. }
        )
    }

    pub fn on_click(&self) -> Option<u64> {
        match self {
            ViewNode::Listener { on_click, .. } => *on_click,
            _ => None,
        }
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
                            text: content.clone(),
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
                            data: std::sync::Arc::new(data.clone()),
                            width: *w,
                            height: *h,
                            opacity: None,
                        },
                        z_index,
                    )
                    .with_id(element_id.as_ffi()),
                );
            }
            ViewNode::Box { style, .. } => {
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
                                radius: 0.0,
                                style: FillStrokeStyle::new().with_fill(bg),
                            },
                            z_index,
                        )
                        .with_id(element_id.as_ffi()),
                    );
                }
            }
            ViewNode::Flex { .. } | ViewNode::Listener { .. } => {}
        }
    }
}
