use lieui::app::App;
use lieui::core::ViewContext;
use lieui::geometry::Size;
use lieui::prelude::Color;
use lieui::widgets::{Button, Column, Container, Text};
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

    // 创建按钮并注册点击回调 - 链式调用 API
    let button_id = ctx.create(Button::new("Click Me!").on_click(|btn| {
        println!("Button clicked! Current text: {}", btn.text_content());
        btn.set_text("Clicked!");
    }));
    ctx.add_child(column, button_id);

    // 创建第二个按钮
    let button2_id = ctx.create(Button::new("Reset").on_click(|btn| {
        println!("Reset clicked!");
        btn.set_text("Reset Done");
    }));
    ctx.add_child(column, button2_id);

    ctx.set_root(root);

    // Run the app
    let app = App::new(ctx);
    app.run(event_loop);
}
