use std::rc::Rc;

use crate::event::Event;
use crate::geometry::Color;
use crate::layout::style::FlexStyle;
use crate::layout::types::FlexAlign;
use crate::state::State;
use crate::theme::current;
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::PaintStyle;
use crate::event::EventContext;
use crate::widget::{BuildContext, Widget};

/// 滑块。value 为 [0, 1] 的比例，由外部 `State<f32>` 持有。
///
/// 轨道挂接鼠标事件：按下即按点击位置跳转并捕获鼠标，之后移动实时更新 value，
/// 抬起释放。手柄以「固定宽度的流内元素」夹在填充与占位之间实现近似定位——
/// 因为 flex 布局下填充/占位按 `value : (1 - value)` 瓜分剩余空间，手柄恰好落在
/// 比例分界处，且始终保持在轨道内。
#[derive(Clone)]
pub struct Slider {
    value: State<f32>,
    track_height: f32,
    fill_color: Color,
    handle_color: Color,
    track_color: Color,
}

impl Slider {
    pub fn new(value: State<f32>) -> Self {
        Self {
            value,
            track_height: 6.0,
            fill_color: current().background.brand_default,
            handle_color: Color::WHITE,
            track_color: current().background.secondary_default,
        }
    }
    pub fn track_height(mut self, h: f32) -> Self {
        self.track_height = h;
        self
    }
    pub fn fill_color(mut self, c: Color) -> Self {
        self.fill_color = c;
        self
    }
    pub fn handle_color(mut self, c: Color) -> Self {
        self.handle_color = c;
        self
    }
    pub fn track_color(mut self, c: Color) -> Self {
        self.track_color = c;
        self
    }
}

impl Widget for Slider {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let v = (*self.value.get()).clamp(0.0, 1.0);
        let h = self.track_height;
        let handle = h + 8.0;
        let r = h / 2.0;

        let dragging = ctx.use_state::<bool>(|| false);
        let value = self.value.clone();
        let dragging_c = dragging.clone();
        let dragging_m = dragging.clone();
        let value_m = self.value.clone();

        let on_down = Rc::new(move |ctx: &mut EventContext| {
            if let Some(Event::MouseDown { x, .. }) = ctx.event() {
                let rect = match ctx.current_rect() {
                    Some(r) => r,
                    None => return,
                };
                let w = rect.width;
                if w <= 0.0 {
                    return;
                }
                let ratio = ((x - rect.x) / w).clamp(0.0, 1.0);
                value.set(ratio);
                dragging_c.set(true);
                ctx.capture_mouse();
            }
        });
        let on_move = Rc::new(move |ctx: &mut EventContext| {
            if !*dragging_m.get() {
                return;
            }
            if let Some(Event::MouseMove { x, .. }) = ctx.event() {
                let rect = match ctx.current_rect() {
                    Some(r) => r,
                    None => return,
                };
                let w = rect.width;
                if w <= 0.0 {
                    return;
                }
                let ratio = ((x - rect.x) / w).clamp(0.0, 1.0);
                value_m.set(ratio);
            }
        });
        let on_up = Rc::new(move |_ctx: &mut EventContext| {
            dragging.set(false);
        });

        let listeners = vec![
            Listener::on_mouse_down(on_down),
            Listener::on_mouse_move(on_move),
            Listener::on_mouse_up(on_up),
        ];

        // Row 轨道：在 Column 中可能不被拉伸，显式 align_self(Stretch)
        // 使 flex_grow 有机会分配剩余空间，手柄位置才正确。
        // 不开启 clip：手柄（14px）高于轨道（6px），裁剪会导致手柄内容缺失。
        let track = FlexStyle::row()
            .align_self(FlexAlign::Stretch)
            .align_items(FlexAlign::Center)
            .height(h);
        let fill = FlexStyle::default().flex_grow(v).height(h);
        let spacer = FlexStyle::default()
            .flex_grow((1.0_f32 - v).max(0.0_f32))
            .height(h);
        let handle_style = FlexStyle::default().width(handle).height(handle);

        ViewNode::Div {
            layout: track,
            paint: PaintStyle::new()
                .background(self.track_color)
                .radius(r),
            children: vec![
                ViewNode::Div {
                    layout: fill,
                    paint: PaintStyle::new().background(self.fill_color),
                    children: vec![],
                    listeners: vec![],
                    key: None,
                },
                ViewNode::Div {
                    layout: handle_style,
                    paint: PaintStyle::new()
                        .background(self.handle_color)
                        .radius(handle / 2.0)
                        .border(2.0, current().background.brand_default),
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
            listeners,
            key: None,
        }
    }
}
