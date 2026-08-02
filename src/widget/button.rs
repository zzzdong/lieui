use crate::event::EventContext;
use crate::geometry::Color;
use crate::layout::style::FlexStyle;
use crate::layout::types::FlexAlign;
use crate::layout_methods;
use crate::theme;
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::{FontWeight, PaintStyle, TextStyle};
use crate::widget::icon::{Icon, IconName};
use crate::widget::tooltip::Tooltip;
use crate::widget::{BuildContext, Widget};
use std::rc::Rc;

/// hover 回调类型。
type HoverCallback = Rc<dyn Fn(&mut EventContext)>;

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

/// 按钮内部内容。Button 是通用容器，可承载文本、图标或任意 widget。
pub enum ButtonContent {
    /// 文本内容（`Button::new` / `Button::with_text`）。
    Text {
        label: String,
        /// 文本样式自定义（字号、颜色、字重等）。未设置的属性从主题默认值回退。
        style: Option<TextStyle>,
        /// 是否允许文本换行。默认 false。
        wrap: bool,
    },
    /// 图标内容（`Button::icon`）。
    Icon {
        name: IconName,
        size: f64,
        color: Option<Color>,
        hover_color: Option<Color>,
        pressed_color: Option<Color>,
    },
    /// 任意 widget 内容（`Button::child`）。视觉由 widget 自身控制。
    Widget(Box<dyn Widget>),
}

pub struct Button {
    content: ButtonContent,
    listeners: Vec<Listener>,
    /// 通用布局属性（width/height/min_width/flex_shrink 等）。
    layout: crate::widget::layout::LayoutAttr,
    /// 固定边长（图标按钮等正方形外壳用）。设置后忽略 size 的 padding。
    fixed_size: Option<f32>,
    /// 视觉样式自定义（背景色、悬停色、按下色、圆角、边框等）。
    paint: Option<PaintStyle>,
    padding_h: Option<f32>,
    padding_v: Option<f32>,
    /// PatternFly 变体，决定默认配色与边框。
    variant: ButtonVariant,
    /// PatternFly 尺寸，决定 padding 与字号。
    size: ButtonSize,
    /// 工具提示文本（鼠标悬停时显示）。
    tooltip: Option<String>,
    /// 鼠标进入回调。
    on_enter: Option<HoverCallback>,
    /// 鼠标离开回调。
    on_leave: Option<HoverCallback>,
    /// 是否禁用。禁用时不响应点击/悬停，视觉灰显。
    disabled: bool,
}

impl Button {
    /// 创建一个文本按钮。
    pub fn new(l: impl Into<String>) -> Self {
        Self {
            content: ButtonContent::Text {
                label: l.into(),
                style: None,
                wrap: false,
            },
            listeners: Vec::new(),
            // Button 默认不被 flex 压缩（flex_shrink 0）。
            layout: crate::widget::layout::LayoutAttr::new().flex_shrink(0.0),
            fixed_size: None,
            paint: None,
            padding_h: None,
            padding_v: None,
            variant: ButtonVariant::Primary,
            size: ButtonSize::Md,
            tooltip: None,
            on_enter: None,
            on_leave: None,
            disabled: false,
        }
    }

    /// 创建一个图标按钮。图标默认随变体着色，hover 时变品牌色（Primary 为白色）。
    pub fn icon(name: IconName, size: f64) -> Self {
        Self {
            content: ButtonContent::Icon {
                name,
                size,
                color: None,
                hover_color: None,
                pressed_color: None,
            },
            listeners: Vec::new(),
            layout: crate::widget::layout::LayoutAttr::new().flex_shrink(0.0),
            fixed_size: None,
            paint: None,
            padding_h: None,
            padding_v: None,
            variant: ButtonVariant::Plain,
            size: ButtonSize::Md,
            tooltip: None,
            on_enter: None,
            on_leave: None,
            disabled: false,
        }
    }

    /// 创建一个内容为任意 widget 的按钮。
    pub fn child<W: Widget + 'static>(w: W) -> Self {
        Self {
            content: ButtonContent::Widget(Box::new(w)),
            listeners: Vec::new(),
            layout: crate::widget::layout::LayoutAttr::new().flex_shrink(0.0),
            fixed_size: None,
            paint: None,
            padding_h: None,
            padding_v: None,
            variant: ButtonVariant::Plain,
            size: ButtonSize::Md,
            tooltip: None,
            on_enter: None,
            on_leave: None,
            disabled: false,
        }
    }

    /// 设置固定边长（正方形外壳）。主要用于图标按钮；设置后忽略 size 的 padding。
    pub fn fixed_size(mut self, v: f32) -> Self {
        self.fixed_size = Some(v);
        self
    }

    layout_methods!(for Button);

    /// 仅当内容是文本时设置文本样式。非文本内容时忽略。
    pub fn set_text_style(&mut self, s: TextStyle) {
        if let ButtonContent::Text { style, .. } = &mut self.content {
            *style = Some(s);
        }
    }

    /// 图标按钮：显式指定图标颜色（覆盖变体默认）。
    pub fn icon_color(mut self, c: Color) -> Self {
        if let ButtonContent::Icon { color, .. } = &mut self.content {
            *color = Some(c);
        }
        self
    }

    /// 图标按钮：显式指定 hover 时图标颜色。
    pub fn icon_hover_color(mut self, c: Color) -> Self {
        if let ButtonContent::Icon { hover_color, .. } = &mut self.content {
            *hover_color = Some(c);
        }
        self
    }

    /// 图标按钮：显式指定按下时图标颜色。
    pub fn icon_pressed_color(mut self, c: Color) -> Self {
        if let ButtonContent::Icon { pressed_color, .. } = &mut self.content {
            *pressed_color = Some(c);
        }
        self
    }

    /// 设置按钮是否禁用。禁用后不响应点击与悬停，视觉灰显。
    pub fn disabled(mut self, v: bool) -> Self {
        self.disabled = v;
        self
    }

    pub fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_click(Rc::new(f)));
        self
    }

    /// 带上下文参数的点击回调。
    pub fn on_click_with_ctx<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_click_with_ctx(Rc::new(f)));
        self
    }

    /// 图标按钮：设置图标字号（像素）。仅对图标内容生效。
    pub fn icon_size(mut self, v: f64) -> Self {
        if let ButtonContent::Icon { size, .. } = &mut self.content {
            *size = v;
        }
        self
    }

    /// 鼠标进入回调（指针移入按钮区域时触发）。
    pub fn on_mouse_enter<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.on_enter = Some(Rc::new(f));
        self
    }

    /// 鼠标离开回调（指针移出按钮区域时触发）。
    pub fn on_mouse_leave<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.on_leave = Some(Rc::new(f));
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

    /// 完整指定文本样式。会覆盖通过 `.font_size()` / `.color()` 等快捷方式设置的属性。
    /// 仅对文本内容生效。
    pub fn text_style(mut self, s: TextStyle) -> Self {
        self.set_text_style(s);
        self
    }

    /// 文本字号。仅对文本内容生效。
    pub fn font_size(mut self, v: f64) -> Self {
        if let ButtonContent::Text { style, .. } = &mut self.content {
            style.get_or_insert_with(TextStyle::default).font_size = v;
        }
        self
    }

    /// 文本颜色。仅对文本内容生效。
    pub fn color(mut self, c: Color) -> Self {
        if let ButtonContent::Text { style, .. } = &mut self.content {
            style.get_or_insert_with(TextStyle::default).color = c;
        }
        self
    }

    /// 是否允许文本换行。默认 false（按钮文本保持单行）。仅对文本内容生效。
    pub fn wrap(mut self, v: bool) -> Self {
        if let ButtonContent::Text { wrap, .. } = &mut self.content {
            *wrap = v;
        }
        self
    }

    /// 字重。仅对文本内容生效。
    pub fn font_weight(mut self, w: impl Into<FontWeight>) -> Self {
        if let ButtonContent::Text { style, .. } = &mut self.content {
            style.get_or_insert_with(TextStyle::default).font_weight = w.into();
        }
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

    /// 设置工具提示文本（鼠标悬停时显示）。
    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip = Some(text.into());
        self
    }
}

/// 内部包装：将构建好的 Button ViewNode 数据封装为 Widget，供 Tooltip 包裹。
struct ButtonNode {
    layout: FlexStyle,
    paint: PaintStyle,
    children: Vec<ViewNode>,
    listeners: Vec<Listener>,
}

impl Widget for ButtonNode {
    fn build(&self, _ctx: &mut BuildContext) -> ViewNode {
        ViewNode::Div {
            layout: self.layout.clone(),
            paint: self.paint.clone(),
            key: None,
            children: self.children.clone(),
            listeners: self.listeners.clone(),
        }
    }
}

impl Widget for Button {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let t = theme::current();
        let (base_paint, base_text) = resolve_variant(t, self.variant);
        let (def_pv, def_ph, def_font) = button_size_metrics(t, self.size);

        // ── 生成内容节点（文本 / 图标 / 任意 widget）──
        let content_node = self.build_content(ctx, base_text, def_font, t);

        let content = ViewNode::Div {
            layout: FlexStyle::row()
                .justify_content(FlexAlign::Center)
                .align_items(FlexAlign::Center)
                .flex_grow(1.0),
            paint: PaintStyle::default(),
            key: None,
            children: vec![content_node],
            listeners: vec![],
        };

        // ── 外部容器尺寸 ──
        let mut layout = FlexStyle::block();
        let ph = self.padding_h.unwrap_or(def_ph);
        let pv = self.padding_v.unwrap_or(def_pv);
        if let Some(px) = self.fixed_size {
            // 固定边长：忽略 padding，正方形外壳。
            layout = layout.width(px).height(px);
        } else {
            layout = layout
                .padding_left(ph)
                .padding_right(ph)
                .padding_top(pv)
                .padding_bottom(pv);
        }
        // 应用通用布局属性（width/height/min_width/flex_shrink/margin/align 等）。
        layout = self.layout.apply(layout);

        // ── 视觉样式：变体打底 + 用户自定义覆盖 ──
        let mut paint = base_paint;
        if self.disabled {
            // 禁用：背景统一为禁用灰，忽略 hover/pressed，且不挂监听器。
            paint = PaintStyle::new()
                .background(t.background.disabled_default)
                .radius(t.radius.small);
        } else if let Some(custom) = &self.paint {
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

        let btn_layout = layout;
        let btn_paint = paint;
        let btn_children = vec![content];
        let btn_listeners = if self.disabled {
            Vec::new()
        } else {
            let mut l = self.listeners.clone();
            if let Some(cb) = &self.on_enter {
                l.push(Listener::on_mouse_enter(Rc::clone(cb)));
            }
            if let Some(cb) = &self.on_leave {
                l.push(Listener::on_mouse_leave(Rc::clone(cb)));
            }
            l
        };

        let btn_node = ViewNode::Div {
            layout: btn_layout.clone(),
            paint: btn_paint.clone(),
            key: None,
            children: btn_children.clone(),
            listeners: btn_listeners.clone(),
        };

        if let Some(tip) = &self.tooltip {
            let wrapper = ButtonNode {
                layout: btn_layout,
                paint: btn_paint,
                children: btn_children,
                listeners: btn_listeners,
            };
            return ctx.child(0, &Tooltip::new(Box::new(wrapper), tip.clone()));
        }

        btn_node
    }
}

impl Button {
    /// 依据内容类型生成对应的内容 ViewNode。
    fn build_content(
        &self,
        ctx: &mut BuildContext,
        base_text: Color,
        def_font: f64,
        t: theme::Theme,
    ) -> ViewNode {
        match &self.content {
            ButtonContent::Text { label, style, wrap } => {
                let label_style = {
                    let mut s = TextStyle {
                        font_size: def_font,
                        color: if self.disabled {
                            t.text.subtle_default
                        } else {
                            base_text
                        },
                        ..TextStyle::default()
                    };
                    if let Some(custom) = style {
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
                    s.wrap = *wrap;
                    s
                };
                ViewNode::Text {
                    content: label.clone(),
                    style: label_style,
                    layout: FlexStyle::default(),
                    key: None,
                    listeners: vec![],
                }
            }
            ButtonContent::Icon {
                name,
                size,
                color,
                hover_color,
                pressed_color,
            } => {
                // 默认图标色随变体；Plain/Outline hover 变品牌色，Primary 保持白色。
                let is_primary = self.variant == ButtonVariant::Primary;
                let icon_color = color.unwrap_or(base_text);
                let icon_hover = hover_color.unwrap_or(if self.disabled {
                    t.text.subtle_default
                } else if is_primary {
                    Color::WHITE
                } else {
                    t.background.brand_default
                });
                let icon_pressed = pressed_color.unwrap_or(if self.disabled {
                    t.text.subtle_default
                } else if is_primary {
                    Color::WHITE
                } else {
                    t.background.brand_clicked
                });
                let icon = Icon::new(*name, *size)
                    .color(if self.disabled {
                        t.text.subtle_default
                    } else {
                        icon_color
                    })
                    .hover_color(icon_hover)
                    .pressed_color(icon_pressed);
                ctx.child(0, &icon)
            }
            ButtonContent::Widget(w) => ctx.child(0, w.as_ref()),
        }
    }
}
