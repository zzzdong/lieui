use lieui::core::ViewContext;
use lieui::geometry::Size;
use lieui::layout::JustifyContent;
use lieui::widgets::{Container, TextInput};
use winit::event_loop::EventLoop;

fn main() {
    let event_loop = EventLoop::new().unwrap();

    let mut ctx = ViewContext::new(Size::new(800.0, 600.0));
    ctx.debug_render_tree = true;

    let root = ctx.root(Container::new().background("#F0F0F0"));
    let column = ctx.create(
        ctx.column()
            .spacing(20.0)
            .expand(true)
            .justify(JustifyContent::Center),
    );
    ctx.add_child(root, column);

    ctx.attach(column, ctx.text("LieUI Window Demo").font_size(32.0));
    ctx.attach(
        column,
        ctx.text("A minimal Rust GUI library").font_size(16.0),
    );

    let btn = ctx.button("Click Me!").on_click(move |ctx| {
        ctx.request_render();
    });
    ctx.attach(column, btn);

    ctx.attach(column, ctx.text("TextInput Demo:").font_size(18.0));
    ctx.attach(
        column,
        TextInput::new().placeholder("单行输入...").width(300.0),
    );
    ctx.attach(
        column,
        TextInput::new()
            .placeholder("多行输入...\n支持换行")
            .width(300.0)
            .height(150.0)
            .multiline(true),
    );

    ctx.run(event_loop);
}
