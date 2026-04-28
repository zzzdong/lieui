use lieui::app::App;
use lieui::core::ViewContext;
use lieui::geometry::Size;
use lieui::layout::JustifyContent;
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

    // 减号按钮 - 使用 on_click 注册回调
    let count_minus = Rc::clone(&count);
    let count_text_minus = Rc::clone(&ctx.widget_tree().get_widget_ref(count_text).unwrap());
    let btn_minus = ctx.create(Button::new("-").on_click(move || {
        let mut c = count_minus.borrow_mut();
        let old_c = *c;
        *c -= 1;
        // 修改 Text 的内容
        if let Ok(mut text) = count_text_minus.try_borrow_mut() {
            if let Some(text_widget) = text.as_any_mut().downcast_mut::<Text>() {
                text_widget.set_content(c.to_string());
            }
        }
        println!("Minus clicked: {} -> {}", old_c, *c);
    }));
    ctx.add_child(button_row, btn_minus);

    // 加号按钮
    let count_plus = Rc::clone(&count);
    let count_text_plus = Rc::clone(&ctx.widget_tree().get_widget_ref(count_text).unwrap());
    let btn_plus = ctx.create(Button::new("+").on_click(move || {
        let mut c = count_plus.borrow_mut();
        let old_c = *c;
        *c += 1;
        // 修改 Text 的内容
        if let Ok(mut text) = count_text_plus.try_borrow_mut() {
            if let Some(text_widget) = text.as_any_mut().downcast_mut::<Text>() {
                text_widget.set_content(c.to_string());
            }
        }
        println!("Plus clicked: {} -> {}", old_c, *c);
    }));
    ctx.add_child(button_row, btn_plus);

    // 重置按钮
    let count_reset = Rc::clone(&count);
    let count_text_reset = Rc::clone(&ctx.widget_tree().get_widget_ref(count_text).unwrap());
    let btn_reset = ctx.create(Button::new("Reset").on_click(move || {
        let mut c = count_reset.borrow_mut();
        let old_c = *c;
        *c = 0;
        // 修改 Text 的内容
        if let Ok(mut text) = count_text_reset.try_borrow_mut() {
            if let Some(text_widget) = text.as_any_mut().downcast_mut::<Text>() {
                text_widget.set_content(c.to_string());
            }
        }
        println!("Reset clicked: {} -> 0", old_c);
    }));
    ctx.add_child(content, btn_reset);

    ctx.set_root(root);

    let app = App::new(ctx);
    app.run(event_loop);
}
