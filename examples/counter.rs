use lieui::core::ViewContext;
use lieui::geometry::Size;
use lieui::layout::JustifyContent;
use lieui::widgets::{Container, Text};
use winit::event_loop::EventLoop;

fn main() {
    let event_loop = EventLoop::new().unwrap();

    let mut ctx = ViewContext::new(Size::new(800.0, 600.0));
    ctx.debug_render_tree = true;

    let root = ctx.root(Container::new().background("#F5F5F5"));
    let center_column = ctx.create(
        ctx.column()
            .spacing(0.0)
            .expand(true)
            .justify(JustifyContent::Center),
    );
    ctx.add_child(root, center_column);

    let content = ctx.create(ctx.column().spacing(16.0).justify(JustifyContent::Start));
    ctx.add_child(center_column, content);

    ctx.attach(content, ctx.text("Counter").font_size(48.0));

    let count_text = ctx.attach(content, ctx.text("0").font_size(72.0));

    let button_row = ctx.create(ctx.row().spacing(16.0));
    ctx.add_child(content, button_row);

    ctx.attach(
        button_row,
        ctx.button("-").on_click(move |ctx| {
            if let Some(mut text) = ctx.get_mut::<Text>(count_text) {
                let current = text.text_content().parse::<i32>().unwrap_or(0);
                text.set_content((current - 1).to_string());
            }
            ctx.request_render();
        }),
    );

    ctx.attach(
        button_row,
        ctx.button("+").on_click(move |ctx| {
            if let Some(mut text) = ctx.get_mut::<Text>(count_text) {
                let current = text.text_content().parse::<i32>().unwrap_or(0);
                text.set_content((current + 1).to_string());
            }
            ctx.request_render();
        }),
    );

    ctx.attach(
        content,
        ctx.button("Reset").on_click(move |ctx| {
            if let Some(mut text) = ctx.get_mut::<Text>(count_text) {
                text.set_content("0".to_string());
            }
            ctx.request_render();
        }),
    );

    ctx.run(event_loop);
}
