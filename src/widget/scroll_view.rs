use crate::layout::style::FlexStyle;
use crate::state::State;
use crate::theme;
use crate::view::node::ViewNode;
use crate::view::paint::PaintStyle;
use crate::widget::layout::LayoutAttr;
use crate::widget::{BuildContext, Widget};

/// 通用滚动容器（ScrollView）。
///
/// 基于 ViewNode 层 engine 滚动：视口标记 `overflow_scroll`，自身按视口尺寸布局，
/// 子节点在内容画布上自然排布；滚动偏移、内容尺寸计算、裁剪与滚轮处理
/// 全部由布局 / 渲染 / 事件引擎统一完成。
///
/// **ScrollView 不提供内置滚动条。** 滚动条的拇指尺寸需要 build 时就已知视口与内容高度，
/// 但这两个值只有 layout 后才知道，在 Widget 构建层无法精确计算。
/// 如需滚动条，使用 `ScrollBar` 单独放置（与 `VirtualList` 搭配的经典模式）。
///
/// ## 用法
///
/// ```ignore
/// // 固定高度 + 显式内容高度
/// ScrollView::new(200.0).content_height(800.0).child(long_content)
///
/// // 自适应撑满
/// ScrollView::expand().child(long_content)
/// ```
///
/// **滚轮事件由引擎自动处理**（命中检测 → 找最近 overflow_scroll 祖先 → 调用 scroll_by）。
/// widget 层无需手动添加 wheel listener。如需同步偏移到外部（例如 ScrollBar），
/// 使用 `bind_scroll_state`：
///
/// ```ignore
/// let scroll = State::new((0.0, 0.0));
/// ScrollView::new(200.0).bind_scroll_state(&scroll).child(content);
/// ScrollBar::vertical(200.0, scroll, 200.0, total).thickness(8.0);
/// ```
pub struct ScrollView {
    height: Option<f32>,
    layout: LayoutAttr,
    child: Option<Box<dyn Widget>>,
    /// 是否显示引擎层自绘滚动条（默认不显示）。
    show_scrollbar: bool,
    /// 可选的显式内容宽度（引擎默认从子节点计算）。
    content_width: Option<f32>,
    /// 可选的显式内容高度（引擎默认从子节点计算）。
    content_height: Option<f32>,
    /// 引擎绑定的滚动偏移 State（推荐方式）。
    scroll_state: Option<State<(f32, f32)>>,
}

impl ScrollView {
    /// 固定高度视口。
    pub fn new(height: f32) -> Self {
        Self {
            height: Some(height),
            layout: LayoutAttr::new(),
            child: None,
            content_width: None,
            content_height: None,
            scroll_state: None,
            show_scrollbar: false,
        }
    }

    /// 高度跟随父容器可用空间自适应撑满。
    pub fn expand() -> Self {
        Self {
            height: None,
            layout: LayoutAttr::new(),
            child: None,
            content_width: None,
            content_height: None,
            scroll_state: None,
            show_scrollbar: false,
        }
    }

    /// 设置 flex 收缩因子（默认 1.0）。
    pub fn flex_shrink(mut self, v: f32) -> Self {
        self.layout = self.layout.flex_shrink(v);
        self
    }

    /// 用完整布局属性（builder 式）设置本滚动容器的布局。
    pub fn layout(mut self, l: LayoutAttr) -> Self {
        self.layout = l;
        self
    }

    /// 显示/隐藏引擎层自绘滚动条（默认隐藏）。
    /// 引擎在渲染时根据 layout 后的实际视口/内容尺寸精确计算 thumb 位置和大小。
    pub fn scrollbar(mut self, show: bool) -> Self {
        self.show_scrollbar = show;
        self
    }

    /// 设置要滚动的单个子 widget。
    pub fn child(mut self, v: impl Widget + 'static) -> Self {
        self.child = Some(Box::new(v));
        self
    }

    /// 显式设置内容宽度（引擎默认从子节点计算）。
    /// 当子节点未全部构建（如虚拟列表）时必须设置。
    pub fn content_width(mut self, v: f32) -> Self {
        self.content_width = Some(v);
        self
    }

    /// 显式设置内容高度（引擎默认从子节点计算）。
    pub fn content_height(mut self, v: f32) -> Self {
        self.content_height = Some(v);
        self
    }

    /// 绑定 `State<(f32, f32)>`，引擎在滚动时自动同步写入此 State。
    /// 引擎是滚动偏移的单一来源，无漂移风险。
    pub fn bind_scroll_state(mut self, s: &State<(f32, f32)>) -> Self {
        self.scroll_state = Some(s.clone());
        self
    }
}

impl Widget for ScrollView {
    fn build(&self, _ctx: &mut BuildContext) -> ViewNode {
        let t = theme::current();

        let inner = match &self.child {
            Some(c) => _ctx.child(0, c.as_ref()),
            None => ViewNode::Div {
                layout: FlexStyle::block(),
                paint: PaintStyle::default(),
                children: vec![],
                listeners: vec![],
                key: None,
            },
        };

        let mut viewport = match self.height {
            Some(h) => FlexStyle::block().height(h),
            None => FlexStyle::block()
                .flex_grow(1.0)
                .flex_shrink(self.layout.flex_shrink.unwrap_or(1.0)),
        }
        .overflow_scroll()
        .scrollbar(self.show_scrollbar);

        // 应用通用布局属性（width/margin/align 等）。flex_shrink 在 expand 模式已设置。
        if self.height.is_some() {
            viewport = self.layout.apply(viewport);
        }

        // 引擎层滚动条占 8px 宽度，预留空间避免遮挡内容右侧
        if self.show_scrollbar {
            viewport = viewport.padding_right(8.0);
        }

        if let Some(w) = self.content_width {
            viewport = viewport.content_width(w);
        }
        if let Some(h) = self.content_height {
            viewport = viewport.content_height(h);
        }
        if let Some(ref s) = self.scroll_state {
            viewport = viewport.bind_scroll_state(s);
        }

        ViewNode::Div {
            layout: viewport,
            paint: PaintStyle::new()
                .background(t.background.secondary_default)
                .clip(true),
            children: vec![inner],
            listeners: vec![],
            key: None,
        }
    }
}
