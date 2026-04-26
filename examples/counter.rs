use lieui::app::App;
use lieui::core::ViewContext;
use lieui::event::{EventResult, EventType};
use lieui::geometry::Size;
use lieui::prelude::Color;
use lieui::widgets::{Button, Column, Container, Row, Text};
use std::cell::RefCell;
use std::rc::Rc;
use winit::event_loop::EventLoop;

fn main() {
    let event_loop = EventLoop::new().unwrap();

    let mut ctx = ViewContext::new(Size::new(800.0, 600.0));
    ctx.debug_render_tree = true;

    // 共享计数状态
    let count = Rc::new(RefCell::new(0i32));

    // 根容器
    let root =
        ctx.create(Container::new().background(Color::from_hex("#F5F5F5").unwrap_or(Color::BLACK)));

    // 垂直居中列
    let column = ctx.create(Column::new().spacing(24.0).expand(true));
    ctx.add_child(root, column);

    // 标题
    let title = ctx.create(Text::new("Counter").font_size(48.0));
    ctx.add_child(column, title);

    // 计数显示
    let count_text = ctx.create(Text::new("0").font_size(72.0));
    ctx.add_child(column, count_text);

    // 按钮行
    let button_row = ctx.create(Row::new().spacing(16.0));
    ctx.add_child(column, button_row);

    // 减号按钮
    let btn_minus = ctx.create(Button::new("-"));
    ctx.add_child(button_row, btn_minus);

    let count_minus = Rc::clone(&count);
    ctx.register(btn_minus, EventType::Click, move |_id, _event, ctx| {
        let mut c = count_minus.borrow_mut();
        *c -= 1;
        if let Some(mut text) = ctx.get::<Text>(count_text) {
            text.set_content(c.to_string());
        }
        ctx.invalidate_render();
        println!("Count: {}", *c);
        EventResult::Continue
    });

    // 加号按钮
    let btn_plus = ctx.create(Button::new("+"));
    ctx.add_child(button_row, btn_plus);

    let count_plus = Rc::clone(&count);
    ctx.register(btn_plus, EventType::Click, move |_id, _event, ctx| {
        let mut c = count_plus.borrow_mut();
        *c += 1;
        if let Some(mut text) = ctx.get::<Text>(count_text) {
            text.set_content(c.to_string());
        }
        ctx.invalidate_render();
        println!("Count: {}", *c);
        EventResult::Continue
    });

    // 重置按钮
    let btn_reset = ctx.create(Button::new("Reset"));
    ctx.add_child(column, btn_reset);

    let count_reset = Rc::clone(&count);
    ctx.register(btn_reset, EventType::Click, move |_id, _event, ctx| {
        let mut c = count_reset.borrow_mut();
        *c = 0;
        if let Some(mut text) = ctx.get::<Text>(count_text) {
            text.set_content(c.to_string());
        }
        ctx.invalidate_render();
        println!("Count reset to 0");
        EventResult::Continue
    });

    ctx.set_root(root);

    let app = App::new(ctx);
    app.run(event_loop);
}
