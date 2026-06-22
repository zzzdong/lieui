use lieui::core::{ViewContext, WidgetId};
use lieui::geometry::Size;
use lieui::layout::JustifyContent;
use lieui::prelude::Color;
use lieui::widgets::Container;
use winit::event_loop::EventLoop;

fn main() {
    let event_loop = EventLoop::new().unwrap();

    let mut ctx = ViewContext::new(Size::new(800.0, 600.0));
    ctx.debug_render_tree = true;

    let root = ctx.root(Container::new().background("#F5F5F5"));
    let main_column = ctx.create(
        ctx.column()
            .spacing(20.0)
            .expand(true)
            .justify(JustifyContent::Center),
    );
    ctx.add_child(root, main_column);

    ctx.attach(main_column, ctx.text("Modal Dialog Demo").font_size(32.0));
    ctx.attach(
        main_column,
        ctx.text("Click the button below to open a modal dialog")
            .font_size(16.0),
    );

    let modal_root = create_modal_widget(&mut ctx);

    ctx.attach(
        main_column,
        ctx.button("Open Modal").on_click(move |ctx| {
            ctx.show_modal(modal_root);
        }),
    );

    ctx.run(event_loop);
}

/// 创建模态框的 widget（但不显示）
fn create_modal_widget(ctx: &mut ViewContext) -> WidgetId {
    let overlay = ctx.create(Container::new().background(Color::from_rgba8(0, 0, 0, 128)));
    let centering_col = ctx.create(ctx.column().expand(true).justify(JustifyContent::Center));
    ctx.add_child(overlay, centering_col);

    let modal_content = ctx.create(ctx.container().background(Color::WHITE).padding(24.0));
    ctx.add_child(centering_col, modal_content);

    let modal_column = ctx.create(ctx.column().spacing(16.0));
    ctx.add_child(modal_content, modal_column);

    ctx.attach(modal_column, ctx.text("Modal Dialog").font_size(24.0));
    ctx.attach(
        modal_column,
        ctx.text(
            "This is a modal dialog.\nBackground interaction is blocked while this modal is open.",
        )
        .font_size(14.0),
    );

    let button_row = ctx.create(ctx.row().spacing(12.0));
    ctx.add_child(modal_column, button_row);

    ctx.attach(
        button_row,
        ctx.button("Cancel").on_click(move |ctx| {
            ctx.hide_modal();
        }),
    );

    ctx.attach(
        button_row,
        ctx.button("OK").on_click(move |ctx| {
            ctx.hide_modal();
        }),
    );

    overlay
}
