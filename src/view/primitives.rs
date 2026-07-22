//! View 基本类型实现（v2）— 通过原语组合成高级 widget

use crate::geometry::Color;
use crate::layout::box_model::BoxStyle;
use crate::layout::flex::{AlignItems, FlexDirection, JustifyContent};
use crate::state;
use crate::view::node::ViewNode;
use crate::view::View;

// ===== Text =====
pub struct Text {
    content: String,
    font_size: f64,
    color: Color,
}
impl Text {
    pub fn new(c: impl Into<String>) -> Self {
        Self {
            content: c.into(),
            font_size: 16.0,
            color: Color::BLACK,
        }
    }
    pub fn font_size(mut self, s: f64) -> Self {
        self.font_size = s;
        self
    }
    pub fn color(mut self, c: Color) -> Self {
        self.color = c;
        self
    }
}
impl View for Text {
    fn build(&self) -> ViewNode {
        ViewNode::Text {
            content: self.content.clone(),
            font_size: self.font_size,
            color: self.color,
            key: None,
        }
    }
}

// ===== Image =====
pub struct Image {
    data: Vec<u8>,
    w: u32,
    h: u32,
}
impl Image {
    pub fn from_rgba(data: Vec<u8>, w: u32, h: u32) -> Self {
        Self { data, w, h }
    }
}
impl View for Image {
    fn build(&self) -> ViewNode {
        ViewNode::Image {
            data: self.data.clone(),
            w: self.w,
            h: self.h,
            key: None,
        }
    }
}

// ===== Container =====
pub struct Container {
    expand: bool,
    width: Option<f32>,
    height: Option<f32>,
    background: Option<Color>,
    padding: f32,
    children: Vec<Box<dyn View>>,
}
impl Container {
    pub fn new() -> Self {
        Self {
            expand: false,
            width: None,
            height: None,
            background: None,
            padding: 0.0,
            children: Vec::new(),
        }
    }
    pub fn width(mut self, v: f32) -> Self {
        self.width = Some(v);
        self
    }
    pub fn height(mut self, v: f32) -> Self {
        self.height = Some(v);
        self
    }
    pub fn child(mut self, c: impl View + 'static) -> Self {
        self.children.push(Box::new(c));
        self
    }
    pub fn expand(mut self, v: bool) -> Self {
        self.expand = v;
        self
    }
    pub fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }
    pub fn padding(mut self, v: f32) -> Self {
        self.padding = v;
        self
    }
}
impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}
impl View for Container {
    fn build(&self) -> ViewNode {
        ViewNode::Box {
            style: BoxStyle {
                expand: self.expand,
                background_color: self.background,
                padding: crate::layout::box_model::EdgeInsets::all(self.padding),
                fixed_width: self.width,
                fixed_height: self.height,
                ..BoxStyle::default()
            },
            key: None,
            children: self.children.iter().map(|c| c.build()).collect(),
        }
    }
}

// ===== Column =====
pub struct Column {
    spacing: f32,
    justify: JustifyContent,
    align: AlignItems,
    expand: bool,
    children: Vec<Box<dyn View>>,
}
impl Column {
    pub fn new() -> Self {
        Self {
            spacing: 4.0,
            justify: JustifyContent::Start,
            align: AlignItems::Start,
            expand: false,
            children: Vec::new(),
        }
    }
    pub fn child(mut self, c: impl View + 'static) -> Self {
        self.children.push(Box::new(c));
        self
    }
    pub fn spacing(mut self, s: f32) -> Self {
        self.spacing = s;
        self
    }
    pub fn justify_content(mut self, j: JustifyContent) -> Self {
        self.justify = j;
        self
    }
    pub fn align_items(mut self, a: AlignItems) -> Self {
        self.align = a;
        self
    }
    pub fn expand(mut self, v: bool) -> Self {
        self.expand = v;
        self
    }
    pub fn center(mut self) -> Self {
        self.justify = JustifyContent::Center;
        self.align = AlignItems::Center;
        self.expand = true;
        self
    }
}
impl Default for Column {
    fn default() -> Self {
        Self::new()
    }
}
impl View for Column {
    fn build(&self) -> ViewNode {
        ViewNode::Flex {
            direction: FlexDirection::Column,
            justify: self.justify,
            align: self.align,
            spacing: self.spacing,
            expand: self.expand,
            key: None,
            children: self.children.iter().map(|c| c.build()).collect(),
        }
    }
}

// ===== Row =====
pub struct Row {
    spacing: f32,
    justify: JustifyContent,
    align: AlignItems,
    expand: bool,
    children: Vec<Box<dyn View>>,
}
impl Row {
    pub fn new() -> Self {
        Self {
            spacing: 4.0,
            justify: JustifyContent::Start,
            align: AlignItems::Center,
            expand: false,
            children: Vec::new(),
        }
    }
    pub fn child(mut self, c: impl View + 'static) -> Self {
        self.children.push(Box::new(c));
        self
    }
    pub fn spacing(mut self, s: f32) -> Self {
        self.spacing = s;
        self
    }
    pub fn justify_content(mut self, j: JustifyContent) -> Self {
        self.justify = j;
        self
    }
    pub fn align_items(mut self, a: AlignItems) -> Self {
        self.align = a;
        self
    }
    pub fn expand(mut self, v: bool) -> Self {
        self.expand = v;
        self
    }
    pub fn center(mut self) -> Self {
        self.justify = JustifyContent::Center;
        self.align = AlignItems::Center;
        self.expand = true;
        self
    }
}
impl Default for Row {
    fn default() -> Self {
        Self::new()
    }
}
impl View for Row {
    fn build(&self) -> ViewNode {
        ViewNode::Flex {
            direction: FlexDirection::Row,
            justify: self.justify,
            align: self.align,
            spacing: self.spacing,
            expand: self.expand,
            key: None,
            children: self.children.iter().map(|c| c.build()).collect(),
        }
    }
}

// ===== Button =====
pub struct Button {
    label: String,
    callback_id: Option<u64>,
}
impl Button {
    pub fn new(l: impl Into<String>) -> Self {
        Self {
            label: l.into(),
            callback_id: None,
        }
    }
    pub fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.callback_id = Some(state::register_click(Box::new(f)));
        self
    }
}
impl View for Button {
    fn build(&self) -> ViewNode {
        let label_node = ViewNode::Text {
            content: self.label.clone(),
            font_size: 14.0,
            color: Color::BLACK,
            key: None,
        };
        let content = ViewNode::Flex {
            direction: FlexDirection::Row,
            justify: JustifyContent::Center,
            align: AlignItems::Center,
            spacing: 0.0,
            expand: false,
            key: None,
            children: vec![label_node],
        };
        let box_node = ViewNode::Box {
            style: BoxStyle {
                padding: crate::layout::box_model::EdgeInsets::new(8.0, 8.0, 6.0, 6.0),
                background_color: Some(Color::new(220, 220, 220)),
                hover_background: Some(Color::new(200, 215, 230)),
                pressed_background: Some(Color::new(180, 200, 220)),
                ..BoxStyle::default()
            },
            key: None,
            children: vec![content],
        };
        ViewNode::Listener {
            on_click: self.callback_id,
            key: None,
            child: Box::new(box_node),
        }
    }
}

// ===== Checkbox =====
pub struct Checkbox {
    checked: bool,
    label: String,
    callback_id: Option<u64>,
}
impl Checkbox {
    pub fn new(checked: bool) -> Self {
        Self {
            checked,
            label: String::new(),
            callback_id: None,
        }
    }
    pub fn label(mut self, s: impl Into<String>) -> Self {
        self.label = s.into();
        self
    }
    pub fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.callback_id = Some(state::register_click(Box::new(f)));
        self
    }
}
impl View for Checkbox {
    fn build(&self) -> ViewNode {
        let check_bg = if self.checked {
            Color::new(60, 120, 220)
        } else {
            Color::new(240, 240, 240)
        };
        let check_box = ViewNode::Box {
            style: BoxStyle {
                fixed_width: Some(16.0),
                fixed_height: Some(16.0),
                background_color: Some(check_bg),
                ..BoxStyle::default()
            },
            key: None,
            children: vec![],
        };
        let mut children: Vec<ViewNode> = vec![check_box];
        if !self.label.is_empty() {
            children.push(ViewNode::Text {
                content: self.label.clone(),
                font_size: 14.0,
                color: Color::BLACK,
                key: None,
            });
        }
        let content = ViewNode::Flex {
            direction: FlexDirection::Row,
            justify: JustifyContent::Start,
            align: AlignItems::Center,
            spacing: 8.0,
            expand: false,
            key: None,
            children,
        };
        ViewNode::Listener {
            on_click: self.callback_id,
            key: None,
            child: Box::new(content),
        }
    }
}

// ===== Divider =====
pub struct Divider;
impl View for Divider {
    fn build(&self) -> ViewNode {
        ViewNode::Box {
            style: BoxStyle {
                fixed_height: Some(1.0),
                expand: true,
                background_color: Some(Color::new(200, 200, 200)),
                ..BoxStyle::default()
            },
            key: None,
            children: vec![],
        }
    }
}
