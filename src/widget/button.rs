use crate::geometry::Color;
use crate::layout::style::FlexStyle;
use crate::layout::types::FlexAlign;
use crate::theme;
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::{FontWeight, PaintStyle, TextStyle};
use crate::widget::{BuildContext, Widget};
use std::rc::Rc;

/// PatternFly Button 变体（variant）。决定默认配色与边框。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    /// 实心品牌主色（默认）
    #[default]
    Primary,
    /// 描边：白底 + 品牌色边框与文字
    Secondary,
    /// 透明底 + 品牌色边框与文字（常用于深色背景）
    Tertiary,
    /// 危险操作：实心红
    Danger,
    /// 警告：实心金（深文字）
    Warning,
    /// 文字型按钮（无背景/边框）
    Link,
    /// 朴素按钮（无背景无边框，hover 浅底）
    Plain,
    /// 表单控件按钮（输入框旁）
    Control,
}

/// PatternFly Button 尺寸（size）。决定 padding 与字号。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonSize {
    /// 小：紧凑 padding + 14px 字号
    Sm,
    /// 中（默认）
    #[default]
    Md,
    /// 大：宽松 padding + 16px 字号
    Lg,
}

/// 将 PatternFly 变体解析为默认视觉样式与文本色。
fn resolve_variant(t: theme::Theme, v: ButtonVariant) -> (PaintStyle, Color) {
    let r = t.radius.small;
    match v {
        ButtonVariant::Primary => (
            PaintStyle::new()
                .background(t.background.brand_default)
                .hover_background(t.background.brand_hover)
                .pressed_background(t.background.brand_clicked)
                .radius(r),
            t.text.on_brand_default,
        ),
        ButtonVariant::Secondary => (
            PaintStyle::new()
                .background(t.background.primary_default)
                .hover_background(t.background.secondary_default)
                .pressed_background(t.background.secondary_default)
                .border(1.0, t.background.brand_default)
                .radius(r),
            t.background.brand_default,
        ),
        ButtonVariant::Tertiary => (
            PaintStyle::new()
                .hover_background(t.background.secondary_default)
                .pressed_background(t.background.secondary_default)
                .border(1.0, t.background.brand_default)
                .radius(r),
            t.background.brand_default,
        ),
        ButtonVariant::Danger => (
            PaintStyle::new()
                .background(t.status.danger)
                .hover_background(Color::from_hex("#a30000"))
                .pressed_background(Color::from_hex("#a30000"))
                .radius(r),
            Color::WHITE,
        ),
        ButtonVariant::Warning => (
            PaintStyle::new()
                .background(t.status.warning)
                .hover_background(Color::from_hex("#c98a00"))
                .pressed_background(Color::from_hex("#c98a00"))
                .radius(r),
            t.text.regular_default,
        ),
        ButtonVariant::Link => (
            PaintStyle::new()
                .hover_background(t.background.secondary_default)
                .radius(r),
            t.text.link_default,
        ),
        ButtonVariant::Plain => (
            PaintStyle::new()
                .hover_background(t.background.secondary_default)
                .radius(r),
            t.text.regular_default,
        ),
        ButtonVariant::Control => (
            PaintStyle::new()
                .background(t.background.primary_default)
                .hover_background(t.background.secondary_default)
                .pressed_background(t.background.secondary_default)
                .border(1.0, t.border.default)
                .radius(r),
            t.text.regular_default,
        ),
    }
}

/// 将 PatternFly 尺寸解析为 (垂直 padding, 水平 padding, 字号)。
fn button_size_metrics(t: theme::Theme, s: ButtonSize) -> (f32, f32, f64) {
    match s {
        ButtonSize::Sm => (4.0, 12.0, t.font.sm),
        ButtonSize::Md => (6.0, 16.0, t.font.sm),
        ButtonSize::Lg => (10.0, 24.0, t.font.md),
    }
}

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
    /// PatternFly 变体，决定默认配色与边框。
    variant: ButtonVariant,
    /// PatternFly 尺寸，决定 padding 与字号。
    size: ButtonSize,
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
            variant: ButtonVariant::Primary,
            size: ButtonSize::Md,
        }
    }

    pub fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_click(Rc::new(f)));
        self
    }

    /// PatternFly 变体。
    pub fn variant(mut self, v: ButtonVariant) -> Self {
        self.variant = v;
        self
    }

    /// PatternFly 尺寸。
    pub fn size(mut self, s: ButtonSize) -> Self {
        self.size = s;
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
    pub fn font_weight(mut self, w: impl Into<FontWeight>) -> Self {
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
        self.paint
            .get_or_insert_with(PaintStyle::new)
            .opacity = o.clamp(0.0, 1.0);
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
        let (base_paint, base_text) = resolve_variant(t, self.variant);
        let (def_pv, def_ph, def_font) = button_size_metrics(t, self.size);

        // ── 文本样式：变体打底 + 用户自定义覆盖 ──
        let label_style = {
            let mut s = TextStyle {
                font_size: def_font,
                color: base_text,
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
        let ph = self.padding_h.unwrap_or(def_ph);
        let pv = self.padding_v.unwrap_or(def_pv);
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

        // ── 视觉样式：变体打底 + 用户自定义覆盖 ──
        let mut paint = base_paint;
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
