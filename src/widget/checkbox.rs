use crate::event::EventContext;
use crate::geometry::Color;
use crate::layout::style::FlexStyle;
use crate::layout::types::{FlexAlign, FlexWrap};
use crate::theme;
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::{PaintStyle, TextStyle};
use crate::widget::{BuildContext, Widget};
use std::rc::Rc;

#[derive(Clone)]
pub struct Checkbox {
    checked: bool,
    label: String,
    listeners: Vec<Listener>,
    /// 复选框勾选块的视觉样式自定义。
    check_paint: Option<PaintStyle>,
    /// 标签文本样式自定义。
    text_style: Option<TextStyle>,
}

impl Checkbox {
    pub fn new(checked: bool) -> Self {
        Self {
            checked,
            label: String::new(),
            listeners: Vec::new(),
            check_paint: None,
            text_style: None,
        }
    }

    pub fn label(mut self, s: impl Into<String>) -> Self {
        self.label = s.into();
        self
    }

    pub fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_click(Rc::new(f)));
        self
    }
    pub fn on_click_with_ctx<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_click_with_ctx(Rc::new(f)));
        self
    }

    // ── 勾选块视觉样式 ──

    pub fn check_paint(mut self, p: PaintStyle) -> Self {
        self.check_paint = Some(p);
        self
    }
    pub fn check_background(mut self, c: Color) -> Self {
        self.check_paint
            .get_or_insert_with(PaintStyle::new)
            .background_color = Some(c);
        self
    }
    pub fn check_hover_background(mut self, c: Color) -> Self {
        self.check_paint
            .get_or_insert_with(PaintStyle::new)
            .hover_background = Some(c);
        self
    }
    pub fn check_border(mut self, width: f32, color: Color) -> Self {
        let p = self.check_paint.get_or_insert_with(PaintStyle::new);
        p.border_width = width;
        p.border_color = Some(color);
        self
    }
    pub fn check_radius(mut self, r: f32) -> Self {
        self.check_paint
            .get_or_insert_with(PaintStyle::new)
            .border_radius = r;
        self
    }

    // ── 标签文本样式 ──

    /// 完整指定标签文本样式。
    pub fn text_style(mut self, s: TextStyle) -> Self {
        self.text_style = Some(s);
        self
    }
    pub fn label_color(mut self, c: Color) -> Self {
        self.text_style.get_or_insert_with(TextStyle::default).color = c;
        self
    }
    pub fn label_font_size(mut self, v: f64) -> Self {
        self.text_style
            .get_or_insert_with(TextStyle::default)
            .font_size = v;
        self
    }
}

impl Widget for Checkbox {
    fn build(&self, _ctx: &mut BuildContext) -> ViewNode {
        let t = theme::current();

        // 勾选块视觉：主题打底 + 自定义覆盖
        let check_paint = {
            let mut p = PaintStyle::new()
                .background(if self.checked {
                    t.background.brand_default
                } else {
                    t.background.primary_default
                })
                .hover_background(if self.checked {
                    t.background.brand_hover
                } else {
                    t.background.secondary_default
                })
                .border(1.0, t.border.strong)
                .radius(t.radius.small);
            if let Some(custom) = &self.check_paint {
                if let Some(bg) = custom.background_color {
                    p.background_color = Some(bg);
                }
                if let Some(hbg) = custom.hover_background {
                    p.hover_background = Some(hbg);
                }
                if let Some(bc) = custom.border_color {
                    p.border_color = Some(bc);
                    p.border_width = custom.border_width;
                }
                if custom.border_radius != PaintStyle::new().border_radius {
                    p.border_radius = custom.border_radius;
                }
            }
            p
        };
        let check_box = ViewNode::Div {
            layout: FlexStyle::default().width(16.0).height(16.0),
            paint: check_paint,
            key: None,
            children: vec![],
            listeners: vec![],
        };

        let mut children = vec![check_box];
        if !self.label.is_empty() {
            let label_style = {
                let mut s = TextStyle {
                    font_size: 13.0,
                    color: t.text.regular_default,
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
                }
                s
            };
            children.push(ViewNode::Text {
                content: self.label.clone(),
                style: label_style,
                layout: FlexStyle::default(),
                key: None,
                listeners: vec![],
            });
        }
        ViewNode::Div {
            layout: FlexStyle::row()
                .justify_content(FlexAlign::Start)
                .align_items(FlexAlign::Center)
                .gap(t.spacer.sm)
                .flex_shrink(1.0)
                .wrap(FlexWrap::NoWrap),
            paint: PaintStyle::default(),
            key: None,
            children,
            listeners: self.listeners.clone(),
        }
    }
}
