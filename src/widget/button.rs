use crate::geometry::Color;
use crate::layout::style::FlexStyle;
use crate::layout::types::FlexAlign;
use crate::theme;
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::{PaintStyle, TextStyle};
use crate::widget::{BuildContext, Widget};
use std::rc::Rc;

pub struct Button {
    label: String,
    listeners: Vec<Listener>,
    width: Option<f32>,
    height: Option<f32>,
    min_width: Option<f32>,
    /// 文本样式自定义（字号、颜色、字重等）。未设置的属性从主题默认值回退。
    text_style: Option<TextStyle>,
    /// 是否允许文本换行。默认 false（按钮文本保持单行）。
    wrap: bool,
    /// 视觉样式自定义（背景色、悬停色、按下色、圆角、边框等）。
    paint: Option<PaintStyle>,
    padding_h: Option<f32>,
    padding_v: Option<f32>,
}

impl Button {
    pub fn new(l: impl Into<String>) -> Self {
        Self {
            label: l.into(),
            listeners: Vec::new(),
            width: None,
            height: None,
            min_width: None,
            text_style: None,
            wrap: false,
            paint: None,
            padding_h: None,
            padding_v: None,
        }
    }

    pub fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_click(Rc::new(f)));
        self
    }

    /// 固定宽度
    pub fn width(mut self, v: f32) -> Self {
        self.width = Some(v);
        self
    }

    /// 固定高度
    pub fn height(mut self, v: f32) -> Self {
        self.height = Some(v);
        self
    }

    /// 最小宽度（不会被 flex 压缩到低于此值）
    pub fn min_width(mut self, v: f32) -> Self {
        self.min_width = Some(v);
        self
    }

    /// 完整指定文本样式。会覆盖通过 `.font_size()` / `.color()` 等快捷方式设置的属性。
    pub fn text_style(mut self, s: TextStyle) -> Self {
        self.text_style = Some(s);
        self
    }

    /// 文本字号
    pub fn font_size(mut self, v: f64) -> Self {
        self.text_style
            .get_or_insert_with(TextStyle::default)
            .font_size = v;
        self
    }

    /// 文本颜色
    pub fn color(mut self, c: Color) -> Self {
        self.text_style.get_or_insert_with(TextStyle::default).color = c;
        self
    }

    /// 是否允许文本换行。默认 false（按钮文本保持单行）。
    pub fn wrap(mut self, v: bool) -> Self {
        self.wrap = v;
        self
    }

    /// 字重
    pub fn font_weight(mut self, w: impl Into<crate::view::paint::FontWeight>) -> Self {
        self.text_style
            .get_or_insert_with(TextStyle::default)
            .font_weight = w.into();
        self
    }

    // ── 视觉样式 ──

    /// 完整指定视觉样式。会覆盖通过 `.background()` / `.radius()` 等快捷方式设置的属性。
    pub fn paint_style(mut self, p: PaintStyle) -> Self {
        self.paint = Some(p);
        self
    }

    /// 背景色
    pub fn background(mut self, c: Color) -> Self {
        self.paint
            .get_or_insert_with(PaintStyle::new)
            .background_color = Some(c);
        self
    }

    /// 悬停背景色
    pub fn hover_background(mut self, c: Color) -> Self {
        self.paint
            .get_or_insert_with(PaintStyle::new)
            .hover_background = Some(c);
        self
    }

    /// 按下背景色
    pub fn pressed_background(mut self, c: Color) -> Self {
        self.paint
            .get_or_insert_with(PaintStyle::new)
            .pressed_background = Some(c);
        self
    }

    /// 圆角半径
    pub fn radius(mut self, r: f32) -> Self {
        self.paint.get_or_insert_with(PaintStyle::new).border_radius = r;
        self
    }

    /// 边框（宽度 + 颜色）
    pub fn border(mut self, width: f32, color: Color) -> Self {
        let p = self.paint.get_or_insert_with(PaintStyle::new);
        p.border_width = width;
        p.border_color = Some(color);
        self
    }

    /// 不透明度
    pub fn opacity(mut self, o: f32) -> Self {
        self.paint.get_or_insert_with(PaintStyle::new).opacity = o.clamp(0.0, 1.0);
        self
    }

    // ── padding ──

    /// 水平 padding（覆盖主题默认值）
    pub fn padding_h(mut self, v: f32) -> Self {
        self.padding_h = Some(v);
        self
    }

    /// 垂直 padding（覆盖主题默认值）
    pub fn padding_v(mut self, v: f32) -> Self {
        self.padding_v = Some(v);
        self
    }
}

impl Widget for Button {
    fn build(&self, _ctx: &mut BuildContext) -> ViewNode {
        let t = theme::current();

        // ── 文本样式：主题打底 + 用户自定义覆盖 ──
        let label_style = {
            let mut s = TextStyle {
                font_size: 13.0,
                color: t.text.on_brand_default,
                ..TextStyle::default()
            };
            if let Some(custom) = &self.text_style {
                if custom.font_size != TextStyle::default().font_size {
                    s.font_size = custom.font_size;
                }
                if custom.color != TextStyle::default().color {
                    s.color = custom.color;
                }
                if custom.font_family != TextStyle::default().font_family {
                    s.font_family = custom.font_family.clone();
                }
                if custom.font_weight != TextStyle::default().font_weight {
                    s.font_weight = custom.font_weight.clone();
                }
                if custom.line_height != TextStyle::default().line_height {
                    s.line_height = custom.line_height;
                }
                if custom.max_width != TextStyle::default().max_width {
                    s.max_width = custom.max_width;
                }
                if custom.text_align != TextStyle::default().text_align {
                    s.text_align = custom.text_align;
                }
            }
            s.wrap = self.wrap;
            s
        };
        let label_node = ViewNode::Text {
            content: self.label.clone(),
            style: label_style,
            layout: FlexStyle::default(),
            key: None,
            listeners: Vec::new(),
        };
        let content = ViewNode::Div {
            layout: FlexStyle::row()
                .justify_content(FlexAlign::Center)
                .align_items(FlexAlign::Center)
                .flex_grow(1.0),
            paint: PaintStyle::default(),
            key: None,
            children: vec![label_node],
            listeners: Vec::new(),
        };

        // ── 外部容器尺寸 ──
        let mut layout = FlexStyle::block();
        let ph = self.padding_h.unwrap_or(t.spacer.md);
        let pv = self.padding_v.unwrap_or(6.0);
        layout = layout
            .padding_left(ph)
            .padding_right(ph)
            .padding_top(pv)
            .padding_bottom(pv);
        if let Some(w) = self.width {
            layout = layout.width(w);
        }
        if let Some(h) = self.height {
            layout = layout.height(h);
        }
        if let Some(mw) = self.min_width {
            layout = layout.min_width(mw);
        }

        // ── 视觉样式：主题打底 + 用户自定义覆盖 ──
        let mut paint = PaintStyle::new()
            .background(t.background.brand_default)
            .hover_background(t.background.brand_hover)
            .pressed_background(t.background.brand_clicked)
            .radius(t.radius.small);
        if let Some(custom) = &self.paint {
            if custom.background_color.is_some() {
                paint.background_color = custom.background_color;
            }
            if custom.hover_background.is_some() {
                paint.hover_background = custom.hover_background;
            }
            if custom.pressed_background.is_some() {
                paint.pressed_background = custom.pressed_background;
            }
            if custom.border_color.is_some() {
                paint.border_color = custom.border_color;
                paint.border_width = custom.border_width;
            }
            if custom.border_radius != PaintStyle::new().border_radius {
                paint.border_radius = custom.border_radius;
            }
            if custom.opacity != PaintStyle::new().opacity {
                paint.opacity = custom.opacity;
            }
            paint.clip_content = custom.clip_content;
        }

        ViewNode::Div {
            layout,
            paint,
            key: None,
            children: vec![content],
            listeners: self.listeners.clone(),
        }
    }
}
