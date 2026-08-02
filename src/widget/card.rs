use crate::layout::style::FlexStyle;
use crate::layout::types::FlexAlign;
use crate::theme::{Theme, current};
use crate::view::node::ViewNode;
use crate::view::paint::{FontWeight, PaintStyle, TextStyle};
use crate::widget::layout::LayoutAttr;
use crate::widget::{BuildContext, Widget};

/// 卡片容器：提供三个具名插槽 `header` / `body` / `footer`，以及可选的标题文本。
///
/// 这等价于 Vue 的 `<Card><template #header>…</template>…<template #footer>…</template></Card>`
/// 在 LieUI 中以「多个 `Vec<Box<dyn Widget>>` 字段 + 对应 builder 方法」实现：
///
/// ```ignore
/// Card::new()
///     .title("用户信息")
///     .header(Text::new("副标题"))
///     .body(Row::new().child(avatar).child(name))
///     .footer(Button::new("确定"))
/// ```
///
/// 不同插槽的子 widget 通过互不重叠的索引偏移（`0` / `1000` / `2000`）生成 `BuildContext`
/// 路径，从而避免嵌套状态（`use_state`）的 key 冲突。
/// PatternFly Card 变体。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CardVariant {
    /// 默认卡片
    #[default]
    Default,
    /// 紧凑卡片（更小的内边距）
    Compact,
    /// 可选中卡片（hover 高亮，点击交互）
    Selectable,
}

pub struct Card {
    title: Option<String>,
    header: Vec<Box<dyn Widget>>,
    body: Vec<Box<dyn Widget>>,
    footer: Vec<Box<dyn Widget>>,
    layout: LayoutAttr,
    variant: CardVariant,
}

impl Card {
    pub fn new() -> Self {
        Self {
            title: None,
            header: Vec::new(),
            body: Vec::new(),
            footer: Vec::new(),
            layout: LayoutAttr::new().padding(16.0),
            variant: CardVariant::Default,
        }
    }

    /// 顶部标题文本（渲染进 header 插槽最上方，加粗展示）。
    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = Some(t.into());
        self
    }

    /// header 插槽：追加一个子 widget（等价于 Vue `<template #header>` 的内容）。
    pub fn header(mut self, w: impl Widget + 'static) -> Self {
        self.header.push(Box::new(w));
        self
    }

    /// body 插槽：卡片主体内容。
    pub fn body(mut self, w: impl Widget + 'static) -> Self {
        self.body.push(Box::new(w));
        self
    }

    /// footer 插槽：底部操作区（通常放按钮）。
    pub fn footer(mut self, w: impl Widget + 'static) -> Self {
        self.footer.push(Box::new(w));
        self
    }

    /// 卡片内边距（默认 16）。
    pub fn padding(mut self, p: f32) -> Self {
        self.layout = self.layout.padding(p);
        self
    }

    /// 用完整布局属性（builder 式）设置本卡片的布局。
    pub fn layout(mut self, l: LayoutAttr) -> Self {
        self.layout = l;
        self
    }

    /// PatternFly 变体（Default / Compact / Selectable）。
    pub fn variant(mut self, v: CardVariant) -> Self {
        self.variant = v;
        self
    }

    /// 设置 flex 收缩因子（默认 1.0）。
    pub fn flex_shrink(mut self, v: f32) -> Self {
        self.layout = self.layout.flex_shrink(v);
        self
    }
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Card {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let t = current();
        let mut children: Vec<ViewNode> = Vec::new();

        // ---- header 插槽 ----
        if self.title.is_some() || !self.header.is_empty() {
            let mut header_children: Vec<ViewNode> = Vec::new();
            if let Some(title) = &self.title {
                header_children.push(ViewNode::Text {
                    content: title.clone(),
                    style: TextStyle {
                        font_size: t.font.lg,
                        color: t.text.regular_default,
                        font_weight: FontWeight::Bold,
                        ..Default::default()
                    },
                    layout: FlexStyle::default(),
                    key: None,
                    listeners: vec![],
                });
            }
            for (i, w) in self.header.iter().enumerate() {
                header_children.push(ctx.child(i, w.as_ref()));
            }
            children.push(ViewNode::Div {
                layout: FlexStyle::column().gap(8.0).padding_bottom(12.0),
                paint: PaintStyle::new(),
                children: header_children,
                listeners: vec![],
                key: Some("header".into()),
            });
            children.push(separator(t));
        }

        // ---- body 插槽 ----
        if !self.body.is_empty() {
            let mut body_children: Vec<ViewNode> = Vec::new();
            for (i, w) in self.body.iter().enumerate() {
                // 偏移 1000，避免与 header / footer 的索引冲突
                body_children.push(ctx.child(1000 + i, w.as_ref()));
            }
            children.push(ViewNode::Div {
                layout: FlexStyle::column()
                    .gap(8.0)
                    .padding_top(12.0)
                    .padding_bottom(if self.footer.is_empty() { 0.0 } else { 12.0 }),
                paint: PaintStyle::new(),
                children: body_children,
                listeners: vec![],
                key: Some("body".into()),
            });
        }

        // ---- footer 插槽 ----
        if !self.footer.is_empty() {
            children.push(separator(t));
            let mut footer_children: Vec<ViewNode> = Vec::new();
            for (i, w) in self.footer.iter().enumerate() {
                // 偏移 2000，避免与其它插槽的索引冲突
                footer_children.push(ctx.child(2000 + i, w.as_ref()));
            }
            children.push(ViewNode::Div {
                layout: FlexStyle::row()
                    .align_items(FlexAlign::Center)
                    .gap(8.0)
                    .padding_top(12.0),
                paint: PaintStyle::new(),
                children: footer_children,
                listeners: vec![],
                key: Some("footer".into()),
            });
        }

        let pad = match self.variant {
            CardVariant::Compact => 12.0,
            _ => self.layout.padding,
        };
        let mut card_paint = PaintStyle::new()
            .background(t.background.primary_default)
            .border(1.0, t.border.default)
            .radius(t.radius.medium)
            .shadow(t.shadow.sm);
        if self.variant == CardVariant::Selectable {
            card_paint = card_paint.hover_background(t.background.secondary_default);
        }

        let mut layout = FlexStyle::column().padding_all(pad);
        layout = self.layout.apply(layout);
        ViewNode::Div {
            layout,
            paint: card_paint,
            children,
            listeners: vec![],
            key: None,
        }
    }
}

/// 1px 分隔线（利用主题边框色）。
fn separator(t: Theme) -> ViewNode {
    ViewNode::Div {
        layout: FlexStyle::default().height(1.0),
        paint: PaintStyle::new().background(t.border.default),
        children: vec![],
        listeners: vec![],
        key: None,
    }
}
