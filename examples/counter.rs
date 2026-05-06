use lieui::app::App;
use lieui::core::ViewContext;
use lieui::geometry::Size;
use lieui::layout::JustifyContent;
use lieui::prelude::Color;
use lieui::widgets::{Button, Column, Container, Row, Text};
use winit::event_loop::EventLoop;

fn main() {
    let event_loop = EventLoop::new().unwrap();

    let mut ctx = ViewContext::new(Size::new(800.0, 600.0));
    ctx.debug_render_tree = true;

    // 根容器 - 浅灰色背景
    let root =
        ctx.create(Container::new().background(Color::from_hex("#F5F5F5").unwrap_or(Color::BLACK)));

    // 使用 Column 居中显示内容
    let center_column = ctx.create(
        Column::new()
            .spacing(0.0)
            .expand(true)
            .justify(JustifyContent::Center),
    );
    ctx.add_child(root, center_column);

    // 内容容器 - 包裹所有控件，不扩展
    let content = ctx.create(Column::new().spacing(16.0).justify(JustifyContent::Start));
    ctx.add_child(center_column, content);

    // 标题
    let title = ctx.create(Text::new("Counter").font_size(48.0));
    ctx.add_child(content, title);

    // 计数显示
    let count_text = ctx.create(Text::new("0").font_size(72.0));
    ctx.add_child(content, count_text);

    // 按钮行 - 不扩展，只包裹内容
    let button_row = ctx.create(Row::new().spacing(16.0));
    ctx.add_child(content, button_row);

    // 减号按钮 - 使用 EventContext 直接访问 Widget
    let btn_minus = ctx.create(Button::new("-").on_click(move |ctx| {
        if let Some(mut text) = ctx.get_mut::<Text>(count_text) {
            let current = text.text_content().parse::<i32>().unwrap_or(0);
            let new_val = current - 1;
            text.set_content(new_val.to_string());
        }
        ctx.request_render();
    }));
    ctx.add_child(button_row, btn_minus);

    // 加号按钮
    let btn_plus = ctx.create(Button::new("+").on_click(move |ctx| {
        if let Some(mut text) = ctx.get_mut::<Text>(count_text) {
            let current = text.text_content().parse::<i32>().unwrap_or(0);
            let new_val = current + 1;
            text.set_content(new_val.to_string());
        }
        ctx.request_render();
    }));
    ctx.add_child(button_row, btn_plus);

    // 重置按钮
    let btn_reset = ctx.create(Button::new("Reset").on_click(move |ctx| {
        if let Some(mut text) = ctx.get_mut::<Text>(count_text) {
            let current = text.text_content().parse::<i32>().unwrap_or(0);
            text.set_content("0".to_string());
        }
        ctx.request_render();
    }));
    ctx.add_child(content, btn_reset);

    ctx.set_root(root);

    let app = App::new(ctx);
    app.run(event_loop);
}
