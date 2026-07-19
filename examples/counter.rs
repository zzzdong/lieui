use lieui::core::ViewContext;
use lieui::geometry::Size;
use lieui::layout::JustifyContent;
use lieui::widgets::Container;
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

    // 使用 State + bind_text，无需手动维护 count_text 的 WidgetId
    let count = ctx.state(0);
    let count_text = ctx.attach(content, ctx.text("0").font_size(72.0));
    ctx.bind_text(&count, count_text, |v| v.to_string());

    let button_row = ctx.create(ctx.row().spacing(16.0));
    ctx.add_child(content, button_row);

    ctx.attach(
        button_row,
        ctx.button("-").on_click({
            let count = count.clone();
            move |_ctx| {
                count.update(|v| *v -= 1);
            }
        }),
    );

    ctx.attach(
        button_row,
        ctx.button("+").on_click({
            let count = count.clone();
            move |_ctx| {
                count.update(|v| *v += 1);
            }
        }),
    );

    ctx.attach(
        content,
        ctx.button("Reset").on_click({
            let count = count.clone();
            move |_ctx| {
                count.set(0);
            }
        }),
    );

    ctx.run(event_loop);
}
