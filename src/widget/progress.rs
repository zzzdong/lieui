use crate::geometry::Color;
use crate::layout::style::FlexStyle;
use crate::layout::types::FlexAlign;
use crate::theme::current;
use crate::view::node::ViewNode;
use crate::view::paint::PaintStyle;
use crate::widget::BuildContext;
use crate::widget::Widget;
use crate::widget::layout::LayoutAttr;

/// 进度条（确定型，value ∈ [0, 1]）。
///
/// 轨道按行布局，填充与占位分别用 `flex_grow(value)` /
/// `flex_grow(1 - value)` 按比例瓜分轨道宽度，因此无需知道父容器像素宽度即可
/// 自适应。轨道设置 `align_self(Stretch)` 以在 Column 等非 Stretch 父容器中仍能
/// 撑满宽度；`clip(true)` 使填充始终裁剪在轨道圆角矩形内。
#[derive(Clone)]
pub struct Progress {
    value: f64,
    layout: LayoutAttr,
    track_color: Color,
    fill_color: Color,
}

impl Progress {
    pub fn new(value: f64) -> Self {
        Self {
            value: value.clamp(0.0, 1.0),
            layout: LayoutAttr::new().height(8.0),
            track_color: current().background.secondary_default,
            fill_color: current().background.brand_default,
        }
    }
    pub fn value(mut self, v: f64) -> Self {
        self.value = v.clamp(0.0, 1.0);
        self
    }
    pub fn height(mut self, h: f32) -> Self {
        self.layout = self.layout.height(h);
        self
    }
    /// 用完整布局属性（builder 式）设置本进度条的布局。
    pub fn layout(mut self, l: LayoutAttr) -> Self {
        self.layout = l;
        self
    }
    pub fn track_color(mut self, c: Color) -> Self {
        self.track_color = c;
        self
    }
    pub fn fill_color(mut self, c: Color) -> Self {
        self.fill_color = c;
        self
    }
}

impl Widget for Progress {
    fn build(&self, _ctx: &mut BuildContext) -> ViewNode {
        let v = self.value.clamp(0.0, 1.0) as f32;
        let h = self.layout.height.unwrap_or(8.0);
        let r = h / 2.0;

        let track = FlexStyle::row().align_self(FlexAlign::Stretch).height(h);
        let fill = FlexStyle::default().flex_grow(v.max(0.0)).height(h);
        let spacer = FlexStyle::default().flex_grow((1.0 - v).max(0.0)).height(h);

        ViewNode::Div {
            layout: track,
            paint: PaintStyle::new()
                .background(self.track_color)
                .radius(r)
                .clip(true),
            children: vec![
                ViewNode::Div {
                    layout: fill,
                    paint: PaintStyle::new().background(self.fill_color),
                    children: vec![],
                    listeners: vec![],
                    key: None,
                },
                ViewNode::Div {
                    layout: spacer,
                    paint: PaintStyle::new(),
                    children: vec![],
                    listeners: vec![],
                    key: None,
                },
            ],
            listeners: vec![],
            key: None,
        }
    }
}
