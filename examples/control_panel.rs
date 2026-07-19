//! Control Panel 示例
//!
//! 展示 LieUI 中多种 widget 的组合使用：
//! - Container / Column / Row 布局
//! - Text、Divider 装饰
//! - Switch、Checkbox 状态切换
//! - Slider 控制数值
//! - ProgressBar 显示进度
//! - TextInput 输入
//! - State<T> + bind_text/bind_progress 自动同步

use lieui::core::ViewContext;
use lieui::geometry::Size;
use lieui::layout::{AlignItems, JustifyContent};
use lieui::prelude::Color;
use lieui::render::visual::BoxShadowDef;
use lieui::widgets::{Checkbox, Container, Divider, ProgressBar, Slider, Switch, TextInput};
use winit::event_loop::EventLoop;

fn main() {
    let event_loop = EventLoop::new().unwrap();

    let mut ctx = ViewContext::new(Size::new(900.0, 700.0));
    ctx.debug_render_tree = true;

    // 共享状态
    let dark_mode = ctx.state(false);
    let notifications = ctx.state(false);
    let volume = ctx.state(50.0f32);

    // 根容器：浅灰背景
    let root = ctx.root(Container::new().background("#F0F2F5"));

    // 居中列
    let center_column = ctx.create(
        ctx.column()
            .spacing(0.0)
            .expand(true)
            .justify(JustifyContent::Center),
    );
    ctx.add_child(root, center_column);

    // 白色卡片：圆角 + 阴影
    let card = ctx.create(
        Container::new()
            .background(Color::WHITE)
            .padding(32.0)
            .border_radius(8.0)
            .box_shadow(BoxShadowDef::new(Color::from_rgba8(0, 0, 0, 64))),
    );
    ctx.add_child(center_column, card);

    // 内容列：交叉轴拉伸，让 Text 等子元素占满整行，避免状态文本换行
    let content = ctx.create(
        ctx.column()
            .spacing(20.0)
            .justify(JustifyContent::Start)
            .align(AlignItems::Stretch),
    );
    ctx.add_child(card, content);

    // 标题栏
    let header = ctx.create(ctx.row().spacing(16.0));
    ctx.add_child(content, header);
    ctx.attach(header, ctx.text("Control Panel").font_size(28.0));
    ctx.attach(
        header,
        ctx.button("Reset").on_click({
            let dark_mode = dark_mode.clone();
            let notifications = notifications.clone();
            let volume = volume.clone();
            move |_ctx| {
                dark_mode.set(false);
                notifications.set(false);
                volume.set(50.0);
            }
        }),
    );

    ctx.attach(content, Divider::new().thickness(1.0).color("#E0E0E0"));

    // Preferences 区域
    ctx.attach(content, ctx.text("Preferences").font_size(20.0));

    let dark_row = ctx.create(ctx.row().spacing(12.0));
    ctx.add_child(content, dark_row);
    ctx.attach(dark_row, ctx.text("Dark mode").font_size(16.0));
    ctx.attach(
        dark_row,
        Switch::new().on_changed({
            let dark_mode = dark_mode.clone();
            move |on| dark_mode.set(on)
        }),
    );

    let notif_row = ctx.create(ctx.row().spacing(12.0));
    ctx.add_child(content, notif_row);
    ctx.attach(
        notif_row,
        Checkbox::new("Enable notifications").on_changed({
            let notifications = notifications.clone();
            move |checked| notifications.set(checked)
        }),
    );

    // 状态显示文本（自动同步）
    let theme_text = ctx.attach(content, ctx.text("Theme: Light").font_size(14.0));
    ctx.bind_text(&dark_mode, theme_text, |v| {
        if *v {
            "Theme: Dark".to_string()
        } else {
            "Theme: Light".to_string()
        }
    });

    let notif_text = ctx.attach(content, ctx.text("Notifications: Off").font_size(14.0));
    ctx.bind_text(&notifications, notif_text, |v| {
        if *v {
            "Notifications: On".to_string()
        } else {
            "Notifications: Off".to_string()
        }
    });

    ctx.attach(content, Divider::new().thickness(1.0).color("#E0E0E0"));

    // Audio 区域
    ctx.attach(content, ctx.text("Audio").font_size(20.0));

    let volume_text = ctx.attach(content, ctx.text("Volume: 50%").font_size(14.0));
    ctx.bind_text(&volume, volume_text, |v| format!("Volume: {:.0}%", v));

    let volume_progress = ctx.attach(content, ProgressBar::new().progress(0.5));
    ctx.bind_progress(&volume, volume_progress, |v| v / 100.0);

    ctx.attach(
        content,
        Slider::new()
            .range(0.0, 100.0)
            .value(50.0)
            .step(1.0)
            .on_value_changed({
                let volume = volume.clone();
                move |v| volume.set(v)
            }),
    );

    ctx.attach(content, Divider::new().thickness(1.0).color("#E0E0E0"));

    // Profile 区域
    ctx.attach(content, ctx.text("Profile").font_size(20.0));
    ctx.attach(
        content,
        TextInput::new().placeholder("Enter your name").width(320.0),
    );

    // 底部状态（自动同步）
    let footer = ctx.attach(content, ctx.text("Ready").font_size(12.0));
    ctx.bind_text(&volume, footer, |v| {
        format!("System volume set to {:.0}%", v)
    });

    ctx.run(event_loop);
}
