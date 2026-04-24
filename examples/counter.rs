use lieui::core::ViewContext;
use lieui::geometry::{Color, Size};
use lieui::widgets::{Button, Column, Container, Text};

fn main() {
    let mut ctx = ViewContext::new(Size::new(800.0, 600.0));

    // Create UI structure
    let root =
        ctx.create(Container::new().background(Color::from_hex("#F5F5F5").unwrap_or(Color::WHITE)));

    let column = ctx.create(Column::new().spacing(10.0));
    ctx.add_child(root, column);

    let title = ctx.create(Text::new("Counter Example"));
    ctx.add_child(column, title);

    let count_display = ctx.create(Text::new("Count: 0").font_size(24.0));
    ctx.add_child(column, count_display);

    let increment_btn = ctx.create(Button::new("+"));
    ctx.add_child(column, increment_btn);

    let decrement_btn = ctx.create(Button::new("-"));
    ctx.add_child(column, decrement_btn);

    ctx.set_root(root);

    println!("LieUI Counter Example - Render Tree:");
    ctx.debug_render_tree = true;

    if let Some(render_tree) = ctx.render() {
        println!("{}", render_tree.to_xml(0));
    }
}
