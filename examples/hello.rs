use lieui::app::App;
use lieui::core::ViewContext;
use lieui::geometry::Size;
use lieui::prelude::Color;
use lieui::widgets::{Button, Column, Container, Text, TextInput};
use std::cell::RefCell;
use std::rc::Rc;
use winit::event_loop::EventLoop;

fn main() {
    // Create event loop
    let event_loop = EventLoop::new().unwrap();

    // Create UI
    let mut ctx = ViewContext::new(Size::new(800.0, 600.0));
    ctx.debug_render_tree = true;

    // Build UI structure
    let root =
        ctx.create(Container::new().background(Color::from_hex("#F0F0F0").unwrap_or(Color::BLACK)));

    let column = ctx.create(Column::new().spacing(20.0).expand(true));
    ctx.add_child(root, column);

    let title = ctx.create(Text::new("LieUI Window Demo").font_size(32.0));
    ctx.add_child(column, title);

    let subtitle = ctx.create(Text::new("A minimal Rust GUI library").font_size(16.0));
    ctx.add_child(column, subtitle);

    // 创建按钮并注册点击回调 - 新的简化 API
    // 使用 Rc<RefCell<>> 来共享可变状态
    let click_count = Rc::new(RefCell::new(0i32));
    let click_count_clone = Rc::clone(&click_count);

    let button_id = ctx.create(Button::new("Click Me!").on_click(move || {
        let mut count = click_count_clone.borrow_mut();
        *count += 1;
        println!("Button clicked! Count: {}", *count);
    }));
    ctx.add_child(column, button_id);

    // 创建第二个按钮
    let button2_id = ctx.create(Button::new("Reset").on_click(move || {
        let mut count = click_count.borrow_mut();
        *count = 0;
        println!("Reset clicked! Count reset to 0");
    }));
    ctx.add_child(column, button2_id);

    // 添加分隔文本
    let input_label = ctx.create(Text::new("TextInput Demo:").font_size(18.0));
    ctx.add_child(column, input_label);

    // 创建文本输入框（带占位符）
    let text_input = ctx.create(TextInput::new().placeholder("请输入文本...").width(300.0));
    ctx.add_child(column, text_input);

    // 创建第二个文本输入框（带初始值）
    let text_input2 = ctx.create(TextInput::new().text("Hello LieUI!").width(300.0));
    ctx.add_child(column, text_input2);

    ctx.set_root(root);

    // Run the app
    let app = App::new(ctx);
    app.run(event_loop);
}
